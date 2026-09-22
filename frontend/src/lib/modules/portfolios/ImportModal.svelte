<script>
  // Import an operations ledger kept somewhere else — a broker export, a spreadsheet,
  // another tracker — without a per-broker parser.
  //
  // The flow is: pick a file → the server detects a mapping (headers in six languages,
  // plus what the values themselves look like) → **the user validates it against a
  // preview of real operations, and says what each symbol is** → import.
  //
  // Three rails, all on purpose:
  //   - nothing is written before the user validates (analyze is read-only);
  //   - a symbol that does not already match an asset of this portfolio blocks the
  //     import until the user resolves it — no asset is ever invented;
  //   - everything written carries a batch id, so a bad import is reverted whole.
  import Modal from '$lib/ui/Modal.svelte';
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
  import { fileToBase64 } from '$lib/import/file.js';
  import ImportSymbols from './ImportSymbols.svelte';
  import { portfoliosApi, IMPORT_TARGETS, fmtMoney, fmtNum } from './api.js';
  import { t, locale } from '$lib/i18n';

  let {
    open = $bindable(false),
    portfolioId,
    portfolioCurrency = 'USD',
    /** The portfolio's assets, for the "point it at an asset I already hold" path. */
    assets = [],
    onimported = () => {}
  } = $props();

  let step = $state('pick'); // pick | map | done
  let busy = $state(false);
  let previewing = $state(false);
  let error = $state('');

  // The file stays in the browser: every call ships it again, so there is no
  // server-side upload state to expire, resume or clean up.
  let file = $state(null); // { name, content }
  let analysis = $state(null);
  let mapping = $state(null);
  // What the detector proposed, kept so the confidence dots survive the user's edits.
  let detected = $state({});
  let saveName = $state('');
  let report = $state(null);

  let batches = $state([]);
  let mappings = $state([]);
  let shelf = $state('history');
  const shelfTabs = $derived([
    { id: 'history', label: $t('portfolios.import.history.title') },
    { id: 'mappings', label: $t('portfolios.import.mappings.title') }
  ]);

  $effect(() => {
    if (open) loadHistory();
  });

  async function loadHistory() {
    [batches, mappings] = await Promise.all([
      portfoliosApi.listImportBatches(portfolioId).catch(() => []),
      portfoliosApi.listImportMappings().catch(() => [])
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
      queue(); // the defaults just applied (timezone) need one round-trip to show
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }

  /** Run a fresh detection (no mapping sent, so the server proposes one) and adopt it. */
  async function detectInto({ section }) {
    const a = await portfoliosApi.importAnalyze(portfolioId, {
      filename: file.name,
      content: file.content,
      section
    });
    analysis = a;
    detected = Object.fromEntries(
      a.columns.map((c) => [c.index, { target: c.target, confidence: c.confidence }])
    );
    // Read naive timestamps in the reader's own zone: an operation keeps its day, and a
    // late-evening fill must not land on the wrong one.
    mapping = { ...a.mapping, tz_offset: -new Date().getTimezoneOffset() };
  }

  // ── Re-preview on every mapping edit (debounced) ──
  let timer = null;
  function queue() {
    clearTimeout(timer);
    timer = setTimeout(refresh, 300);
  }

  async function refresh() {
    if (!file || !mapping) return;
    previewing = true;
    error = '';
    try {
      analysis = await portfoliosApi.importAnalyze(portfolioId, {
        filename: file.name,
        content: file.content,
        mapping
      });
    } catch (e) {
      error = e.message;
    } finally {
      previewing = false;
    }
  }

  // ── Column mapping ──
  const targets = $derived(
    IMPORT_TARGETS.map((t0) => ({ id: t0.id, label: $t(`portfolios.import.field.${t0.key}`) }))
  );

  const columnLabels = $derived({
    ignore: $t('portfolios.import.target.ignore'),
    noSamples: $t('portfolios.import.columns.noSamples'),
    ambiguousDates: $t('portfolios.import.ambiguousDates'),
    unnamed: (n) => $t('portfolios.import.columns.unnamed', { n }),
    kind: (k) => $t(`portfolios.import.kind.${k}`),
    confidence: (c) => $t(`portfolios.import.confidence.${c}`)
  });
  const formatLabels = $derived({
    section: $t('portfolios.import.section'),
    sectionFlat: $t('portfolios.import.section.flat'),
    dateOrder: $t('portfolios.import.dateOrder'),
    dateOrderAuto: $t('portfolios.import.dateOrder.auto'),
    dateOrderDmy: $t('portfolios.import.dateOrder.dmy'),
    dateOrderMdy: $t('portfolios.import.dateOrder.mdy'),
    dateOrderYmd: $t('portfolios.import.dateOrder.ymd'),
    decimal: $t('portfolios.import.decimal'),
    decimalAuto: $t('portfolios.import.decimal.auto'),
    decimalDot: $t('portfolios.import.decimal.dot'),
    decimalComma: $t('portfolios.import.decimal.comma'),
    timezone: $t('portfolios.import.timezone')
  });

  function selectValue(col) {
    return mapping?.columns?.[String(col.index)] ?? 'ignore';
  }

  /** Show the detector's confidence only while its proposal still stands. */
  function confidenceOf(col) {
    const d = detected[col.index];
    const current = selectValue(col);
    if (!d || d.target !== current || current === 'ignore') return 'none';
    return d.confidence;
  }

  function setTarget(col, value) {
    const next = { ...mapping.columns };
    // Reserved fields are one-to-one: taking one frees whoever held it.
    if (value !== 'ignore') {
      for (const [k, v] of Object.entries(next)) {
        if (v === value && k !== String(col.index)) next[k] = 'ignore';
      }
    }
    next[String(col.index)] = value;
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

  /** The user's answer to "what is this symbol?" — the one thing no file states. */
  function chooseSymbol(source, choice) {
    const key = source.trim().toUpperCase();
    const next = { ...(mapping.symbols ?? {}) };
    if (choice) next[key] = choice;
    else delete next[key];
    mapping = { ...mapping, symbols: next };
    queue();
  }

  async function applySavedMapping(id) {
    const saved = mappings.find((m) => m.id === id);
    if (!saved) return;
    // The header row and delimiter belong to this file, not to the saved mapping; the
    // symbol choices belong to this portfolio, so they are re-resolved rather than kept.
    mapping = {
      ...saved.mapping,
      header_row: mapping.header_row,
      delimiter: mapping.delimiter,
      symbols: {}
    };
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
        label: $t('portfolios.import.saveMapping'),
        value: saveName || analysis?.mapping_match?.name || defaultMappingName(),
        placeholder: $t('portfolios.import.saveMapping.placeholder'),
        required: true
      }
    ];
    namePromptOpen = true;
  }

  function defaultMappingName() {
    return (file?.name ?? '').replace(/\.[^.]+$/, '').slice(0, 60);
  }

  function onNameConfirmed({ name }) {
    const trimmed = name.trim();
    if (!trimmed) return;
    const clash = mappings.find((m) => m.name === trimmed && m.name !== saveName);
    if (clash) {
      confirmTitle = $t('portfolios.import.saveMapping.replace.title');
      confirmMessage = $t('portfolios.import.saveMapping.replace.message', { name: trimmed });
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
      // A saved mapping describes the *file*, not the book: the per-symbol choices are
      // left out so the same mapping can feed any portfolio.
      const { symbols: _symbols, ...doc } = mapping;
      await portfoliosApi.saveImportMapping({ name, headers: analysis.headers, mapping: doc });
      saveName = name;
      mappings = await portfoliosApi.listImportMappings();
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }

  // ── Commit ──
  const stats = $derived(analysis?.stats ?? null);
  const importable = $derived(
    stats ? Math.max(0, stats.operations - stats.duplicates - stats.skipped - stats.unresolved) : 0
  );
  const blocked = $derived((stats?.unresolved ?? 0) > 0);

  async function commit() {
    busy = true;
    error = '';
    try {
      report = await portfoliosApi.importCommit(portfolioId, {
        filename: file.name,
        content: file.content,
        mapping,
        save_as: saveName.trim() || null
      });
      step = 'done';
      await loadHistory();
      onimported();
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

  function close() {
    open = false;
    reset();
  }

  // ── History ──
  let confirmOpen = $state(false);
  let confirmTitle = $state('');
  let confirmMessage = $state('');
  let confirmAction = $state(() => {});

  function askRevert(batch) {
    confirmTitle = $t('portfolios.import.history.confirmRevert.title');
    confirmMessage = $t('portfolios.import.history.confirmRevert.message', {
      n: batch.live_operations
    });
    confirmAction = async () => {
      await portfoliosApi.revertImportBatch(batch.id);
      await loadHistory();
      onimported();
    };
    confirmOpen = true;
  }

  function askForget(batch) {
    confirmTitle = $t('portfolios.import.history.confirmForget.title');
    confirmMessage = $t('portfolios.import.history.confirmForget.message');
    confirmAction = async () => {
      await portfoliosApi.forgetImportBatch(batch.id);
      await loadHistory();
    };
    confirmOpen = true;
  }

  function askDeleteMapping(m) {
    confirmTitle = $t('portfolios.import.mappings.confirmDelete.title');
    confirmMessage = $t('portfolios.import.mappings.confirmDelete.message', { name: m.name });
    confirmAction = async () => {
      await portfoliosApi.deleteImportMapping(m.id);
      await loadHistory();
    };
    confirmOpen = true;
  }

  async function revertLast() {
    if (!report?.batch_id) return;
    await portfoliosApi.revertImportBatch(report.batch_id);
    await loadHistory();
    onimported();
    reset();
  }

  const fmtDate = (iso) =>
    iso ? new Date(iso).toLocaleString($locale, { dateStyle: 'medium', timeStyle: 'short' }) : '—';
  const fmtDay = (iso) =>
    iso ? new Date(iso).toLocaleDateString($locale, { dateStyle: 'medium' }) : '—';
</script>

<Modal bind:open size="xl" title={$t('portfolios.import.title')} onclose={reset}>
  {#if step === 'pick'}
    <div class="pick">
      <FileDrop
        title={$t('portfolios.import.pick.drop')}
        hint={$t('portfolios.import.pick.hint')}
        browseLabel={$t('portfolios.import.pick.browse')}
        busyLabel={$t('portfolios.import.analyzing')}
        {busy}
        onpick={pickFile}
      />

      {#if error}
        <p class="error"><Icon name="alert-triangle" size={14} /> {error}</p>
      {/if}

      <section class="shelf">
        <Tabs tabs={shelfTabs} bind:value={shelf} ariaLabel={$t('portfolios.import.history.title')} />

        {#if shelf === 'history'}
          {#if batches.length === 0}
            <EmptyState icon="clock" description={$t('portfolios.import.history.empty')} compact />
          {:else}
            <div class="scroller">
              <table class="tbl">
                <thead>
                  <tr>
                    <th>{$t('portfolios.import.history.file')}</th>
                    <th>{$t('portfolios.import.history.when')}</th>
                    <th>{$t('portfolios.import.history.mapping')}</th>
                    <th class="num">{$t('portfolios.import.history.opsCol')}</th>
                    <th></th>
                  </tr>
                </thead>
                <tbody>
                  {#each batches as b (b.id)}
                    <tr>
                      <td class="strong">{b.filename}</td>
                      <td class="mono">{fmtDate(b.created_at)}</td>
                      <td>{b.mapping_name || '—'}</td>
                      <td class="num">
                        {b.live_operations}
                        {#if b.duplicates > 0}<span class="dim">
                            · {$t('portfolios.import.history.dupes', { n: b.duplicates })}</span
                          >{/if}
                      </td>
                      <td class="row-actions">
                        <Button
                          size="sm"
                          icon="rotate-ccw"
                          variant="danger"
                          disabled={b.live_operations === 0}
                          onclick={() => askRevert(b)}>{$t('portfolios.import.history.revert')}</Button
                        >
                        <Button size="sm" onclick={() => askForget(b)}>
                          {$t('portfolios.import.history.forget')}
                        </Button>
                      </td>
                    </tr>
                  {/each}
                </tbody>
              </table>
            </div>
          {/if}
        {:else if mappings.length === 0}
          <EmptyState
            icon="layers"
            title={$t('portfolios.import.mappings.empty')}
            description={$t('portfolios.import.mappings.hint')}
            compact
          />
        {:else}
          <p class="muted small">{$t('portfolios.import.mappings.hint')}</p>
          <div class="scroller">
            <ul class="maplist">
              {#each mappings as m (m.id)}
                <li>
                  <span class="mapname">{m.name}</span>
                  <span class="dim">
                    {m.last_used_at
                      ? $t('portfolios.import.mappings.used', { date: fmtDay(m.last_used_at) })
                      : $t('portfolios.import.mappings.never')}
                  </span>
                  <button
                    class="icon"
                    title={$t('portfolios.import.mappings.delete')}
                    aria-label={$t('portfolios.import.mappings.delete')}
                    onclick={() => askDeleteMapping(m)}><Icon name="trash" size={14} /></button
                  >
                </li>
              {/each}
            </ul>
          </div>
        {/if}
      </section>
    </div>
  {:else if step === 'map' && analysis && mapping}
    <div class="mapper">
      <header class="bar">
        <div class="file">
          <Icon name="file-text" size={15} />
          <span class="fname">{file.name}</span>
          <span class="dim">{$t('portfolios.import.rows', { n: analysis.row_count })}</span>
          {#if previewing}<span class="dim">{$t('portfolios.import.analyzing')}</span>{/if}
        </div>
        {#if analysis.mapping_match}
          <div class="match">
            <Icon name="sparkles" size={14} />
            <span>{$t('portfolios.import.match', { name: analysis.mapping_match.name })}</span>
            <button class="link" onclick={() => applySavedMapping(analysis.mapping_match.id)}
              >{$t('portfolios.import.match.apply')}</button
            >
          </div>
        {/if}
      </header>

      {#if error}
        <p class="error"><Icon name="alert-triangle" size={14} /> {error}</p>
      {/if}

      <!-- File-wide reading rules: they change how every column is read. -->
      <section class="settings">
        <FormatBar
          {mapping}
          sections={analysis.sections}
          labels={formatLabels}
          onsection={setSection}
          onchange={set}
        />
        <div class="set">
          <span>{$t('portfolios.import.defaultSide')}</span>
          <Dropdown
            value={mapping.defaults.side ?? ''}
            onpick={(v) => setDefault({ side: v })}
            ariaLabel={$t('portfolios.import.defaultSide')}
            options={[
              { value: '', label: $t('portfolios.import.defaultSide.none') },
              { value: 'buy', label: $t('portfolios.import.defaultSide.buy') },
              { value: 'sell', label: $t('portfolios.import.defaultSide.sell') }
            ]}
          />
        </div>
        <div class="conv">
          <label class="check">
            <input
              type="checkbox"
              checked={mapping.conventions.qty_sign_is_side}
              onchange={(e) => setConvention({ qty_sign_is_side: e.target.checked })}
            />
            {$t('portfolios.import.conv.qtySign')}
          </label>
          <label class="check">
            <input
              type="checkbox"
              checked={mapping.conventions.fees_abs}
              onchange={(e) => setConvention({ fees_abs: e.target.checked })}
            />
            {$t('portfolios.import.conv.feesAbs')}
          </label>
        </div>
      </section>

      <div class="split">
        <section class="cols">
          <div class="cols-head">
            <h2>{$t('portfolios.import.columns.title')}</h2>
          </div>
          {#if analysis.preamble.length}
            <p class="preamble">
              {$t('portfolios.import.preamble')}:
              <span class="mono">{analysis.preamble.join(' / ')}</span>
            </p>
          {/if}
          <ColumnList
            columns={analysis.columns}
            {targets}
            value={selectValue}
            confidence={confidenceOf}
            labels={columnLabels}
            onselect={setTarget}
          />
        </section>

        <section class="right">
          <ImportSymbols
            symbols={analysis.symbols}
            {assets}
            {portfolioCurrency}
            onchoose={chooseSymbol}
          />

          <div class="stats">
            <div class="stat">
              <span class="sv">{stats.operations}</span>
              <span class="sl">{$t('portfolios.import.stats.operations')}</span>
            </div>
            <div class="stat">
              <span class="sv">{stats.buys}</span>
              <span class="sl">{$t('portfolios.import.stats.buys')}</span>
            </div>
            <div class="stat">
              <span class="sv">{stats.sells}</span>
              <span class="sl">{$t('portfolios.import.stats.sells')}</span>
            </div>
            {#if stats.new_assets > 0}
              <div class="stat">
                <span class="sv">{stats.new_assets}</span>
                <span class="sl">{$t('portfolios.import.stats.newAssets')}</span>
              </div>
            {/if}
            {#if stats.duplicates > 0}
              <div class="stat">
                <span class="sv">{stats.duplicates}</span>
                <span class="sl">{$t('portfolios.import.stats.duplicates')}</span>
              </div>
            {/if}
            {#if stats.skipped > 0}
              <div class="stat">
                <span class="sv">{stats.skipped}</span>
                <span class="sl">{$t('portfolios.import.stats.skipped')}</span>
              </div>
            {/if}
            {#if stats.unresolved > 0}
              <div class="stat bad">
                <span class="sv">{stats.unresolved}</span>
                <span class="sl">{$t('portfolios.import.stats.unresolved')}</span>
              </div>
            {/if}
            {#if stats.errors > 0}
              <div class="stat bad">
                <span class="sv">{stats.errors}</span>
                <span class="sl">{$t('portfolios.import.stats.errors')}</span>
              </div>
            {/if}
          </div>

          <div class="totals">
            {#each stats.flows as f (f.currency)}
              <span class="total"
                >{$t('portfolios.import.stats.bought')}
                <span class="mono">{fmtMoney(f.bought, f.currency)}</span></span
              >
              {#if f.sold > 0}
                <span class="total"
                  >{$t('portfolios.import.stats.sold')}
                  <span class="mono">{fmtMoney(f.sold, f.currency)}</span></span
                >
              {/if}
            {/each}
            {#if stats.first_date}
              <span class="dim">{fmtDay(stats.first_date)} → {fmtDay(stats.last_date)}</span>
            {/if}
          </div>

          {#if analysis.preview.length === 0}
            <EmptyState icon="alert-triangle" description={$t('portfolios.import.preview.none')} compact />
          {:else}
            <div class="pv-head">
              <h2>{$t('portfolios.import.preview.title')}</h2>
              <span class="dim small"
                >{$t('portfolios.import.preview.showing', {
                  n: analysis.preview.length,
                  total: stats.operations
                })}</span
              >
            </div>
            <div class="pv-scroll">
              <table class="pv">
                <thead>
                  <tr>
                    <th>{$t('portfolios.import.preview.date')}</th>
                    <th>{$t('portfolios.import.preview.asset')}</th>
                    <th>{$t('portfolios.import.preview.side')}</th>
                    <th class="num">{$t('portfolios.import.preview.qty')}</th>
                    <th class="num">{$t('portfolios.import.preview.price')}</th>
                    <th class="num">{$t('portfolios.import.preview.fee')}</th>
                    <th class="num">{$t('portfolios.import.preview.amount')}</th>
                    <th></th>
                  </tr>
                </thead>
                <tbody>
                  {#each analysis.preview as p (p.index)}
                    <tr class:off={p.duplicate || p.skipped} class:pending={p.unresolved}>
                      <td class="mono">{p.op_date}</td>
                      <td>
                        <span class="mono strong">{p.target ?? p.symbol}</span>
                        {#if p.target && p.target !== p.symbol}
                          <span class="dim small">({p.symbol})</span>
                        {/if}
                      </td>
                      <td class="side {p.side}">{$t(`portfolios.detail.${p.side}`)}</td>
                      <td class="num">{fmtNum(p.quantity)}</td>
                      <td class="num">{fmtNum(p.price)}</td>
                      <td class="num">{p.fee ? fmtNum(p.fee) : '—'}</td>
                      <td class="num">{fmtMoney(p.amount, p.currency)}</td>
                      <td class="flags">
                        {#if p.duplicate}<span class="chip">{$t('portfolios.import.preview.duplicate')}</span>{/if}
                        {#if p.skipped}<span class="chip">{$t('portfolios.import.preview.skipped')}</span>{/if}
                        {#if p.unresolved}<span class="chip warn">{$t('portfolios.import.preview.unresolved')}</span>{/if}
                        {#each p.warnings as w (w.code)}
                          <span class="chip warn" title={w.message}>{w.code}</span>
                        {/each}
                      </td>
                    </tr>
                  {/each}
                </tbody>
              </table>
            </div>
          {/if}

          {#if analysis.errors.length}
            <div class="errors">
              <h3><Icon name="alert-triangle" size={13} /> {$t('portfolios.import.errors.title')}</h3>
              <ul>
                {#each analysis.errors.slice(0, 8) as e (e.row)}
                  <li>
                    <span class="eline mono">{$t('portfolios.import.errors.line', { n: e.row })}</span>
                    <span class="emsg">{e.message}</span>
                    <span class="eraw mono">{e.raw}</span>
                  </li>
                {/each}
              </ul>
              {#if analysis.errors.length > 8}
                <p class="dim small">
                  {$t('portfolios.import.errors.more', { n: analysis.errors.length - 8 })}
                </p>
              {/if}
            </div>
          {/if}
        </section>
      </div>
    </div>
  {:else if step === 'done' && report}
    <div class="done">
      <Icon name="check-circle" size={26} />
      <h2>{$t('portfolios.import.done.title')}</h2>
      <p class="summary">
        {$t('portfolios.import.done.summary', {
          imported: report.imported,
          duplicates: report.duplicates,
          failed: report.failed
        })}
      </p>
      {#if report.assets_created > 0}
        <p class="muted">{$t('portfolios.import.done.assets', { n: report.assets_created })}</p>
      {/if}
      <p class="dim small">{$t('portfolios.import.done.revertHint')}</p>
    </div>
  {/if}

  {#snippet footer()}
    {#if step === 'map' && analysis}
      <Button icon="save" onclick={askSaveMapping}>
        {saveName
          ? $t('portfolios.import.saveMapping.saved', { name: saveName })
          : $t('portfolios.import.saveMapping.button')}
      </Button>
      <Button onclick={reset}>{$t('portfolios.import.back')}</Button>
      <Button
        variant="primary"
        icon="upload"
        loading={busy}
        disabled={importable === 0 || blocked}
        onclick={commit}
      >
        {blocked
          ? $t('portfolios.import.commit.blocked', { n: stats.unresolved })
          : importable === 0
            ? $t('portfolios.import.commit.none')
            : $t('portfolios.import.commit', { n: importable })}
      </Button>
    {:else if step === 'done'}
      <Button variant="danger" icon="rotate-ccw" onclick={revertLast}>
        {$t('portfolios.import.done.revert')}
      </Button>
      <Button variant="primary" onclick={close}>{$t('portfolios.import.done.close')}</Button>
    {/if}
  {/snippet}
</Modal>

<PromptModal
  bind:open={namePromptOpen}
  title={$t('portfolios.import.saveMapping.title')}
  fields={namePromptFields}
  confirmLabel={$t('common.save')}
  onconfirm={onNameConfirmed}
/>

<ConfirmModal
  bind:open={confirmOpen}
  title={confirmTitle}
  message={confirmMessage}
  danger
  onconfirm={() => confirmAction()}
/>

<style>
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
  .scroller {
    max-height: 38vh;
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
  .match {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    border-left: 1.5px solid var(--accent);
    background: var(--surface-2);
    padding: var(--space-1) var(--space-3);
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

  .split {
    display: grid;
    grid-template-columns: minmax(300px, 2fr) minmax(420px, 3fr);
    gap: var(--space-6);
    align-items: start;
  }
  @media (max-width: 1100px) {
    .split {
      grid-template-columns: 1fr;
    }
  }
  .right {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-width: 0;
  }
  .cols-head,
  .pv-head {
    display: flex;
    align-items: baseline;
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
  .preamble {
    font-size: var(--text-xs);
    color: var(--dim);
    margin-bottom: var(--space-2);
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
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .total .mono {
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }

  .pv-scroll {
    border: 0.5px solid var(--border);
    max-height: 40vh;
    overflow: auto;
  }
  table.pv {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }
  table.pv th {
    position: sticky;
    top: 0;
    background: var(--surface);
    text-align: left;
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--dim);
    font-weight: var(--fw-medium);
    padding: 4px var(--space-2);
    border-bottom: 0.5px solid var(--border);
  }
  table.pv td {
    padding: 3px var(--space-2);
    border-bottom: 0.5px solid var(--border);
    white-space: nowrap;
  }
  table.pv tbody tr:last-child td {
    border-bottom: none;
  }
  /* Rows that will not be written: still shown, visibly out of the count. */
  table.pv tr.off td {
    opacity: 0.5;
  }
  table.pv tr.pending td {
    background: color-mix(in srgb, var(--amber) 8%, transparent);
  }
  .side {
    text-transform: uppercase;
    font-size: var(--text-xs);
    letter-spacing: 0.04em;
  }
  .side.buy {
    color: var(--green);
  }
  .side.sell {
    color: var(--red);
  }
  .flags {
    display: flex;
    gap: 4px;
  }
  .chip {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
    border: 0.5px solid var(--border);
    padding: 0 var(--space-1);
  }
  .chip.warn {
    color: var(--amber-ink, var(--amber));
    border-color: color-mix(in srgb, var(--amber) 45%, transparent);
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
  .strong {
    font-weight: var(--fw-medium);
  }
  .mono {
    font-family: var(--mono);
  }
  .num {
    text-align: right;
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  .error {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--red);
    font-size: var(--text-sm);
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
  .icon:hover {
    color: var(--text);
  }
</style>
