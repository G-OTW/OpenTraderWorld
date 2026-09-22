<script>
  // Import an OHLCV series the user already has, without a per-vendor parser.
  //
  // The flow is: pick a file → the server detects a mapping (headers in six languages,
  // plus what the values themselves look like) → **the user says what instrument the bars
  // are of and checks them against a preview of real dates and prices** → import.
  //
  // Three rails, all on purpose:
  //   - nothing is written before the user validates (analyze is read-only);
  //   - the file never leaves the browser between steps: every call ships it again, so
  //     there is no server-side upload state to expire, resume or clean up;
  //   - the instrument is asked, never guessed. A file says "Close"; it does not say the
  //     bars are AAPL daily, and an import that invents that is worse than one that asks.
  import Modal from '$lib/ui/Modal.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Input from '$lib/ui/Input.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import TagInput from '$lib/ui/TagInput.svelte';
  import FileDrop from '$lib/import/FileDrop.svelte';
  import FormatBar from '$lib/import/FormatBar.svelte';
  import ColumnList from '$lib/import/ColumnList.svelte';
  import { fileToBase64 } from '$lib/import/file.js';
  import { histdataApi, IMPORT_TARGETS } from './api.js';
  import { fmtNum } from '$lib/format.js';
  import { t } from '$lib/i18n';

  let {
    open = $bindable(false),
    /** Existing datasets, for the asset-type and tag suggestions. */
    datasets = [],
    onimported = () => {}
  } = $props();

  let step = $state('pick'); // pick | map | done
  let busy = $state(false);
  let previewing = $state(false);
  let error = $state('');

  let file = $state(null); // { name, content }
  let analysis = $state(null);
  let mapping = $state(null);
  // What the detector proposed, kept so the confidence dots survive the user's edits.
  let detected = $state({});
  let report = $state(null);

  // What the file is bars *of*. None of it is in the file, so all of it lives here and
  // rides along with every analyze call.
  let dest = $state(blankDest());
  let tagInput = $state(null);

  function blankDest() {
    return { asset_type: '', ticker: '', timeframe: '', source: '', label: '', tags: [] };
  }

  const ASSET_TYPES = ['crypto', 'equity', 'etf', 'fx', 'index', 'future', 'option', 'commodity'];
  const TIMEFRAMES = ['1m', '5m', '15m', '30m', '1h', '4h', '1d', '1w', '1M'];

  const assetSuggestions = $derived([
    ...new Set([...ASSET_TYPES, ...datasets.map((d) => d.asset_type).filter(Boolean)])
  ]);
  const tagSuggestions = $derived([
    ...new Set(datasets.flatMap((d) => (Array.isArray(d.tags) ? d.tags : [])))
  ]);
  const sourceSuggestions = $derived([
    ...new Set(datasets.map((d) => d.source).filter(Boolean))
  ]);

  // ── Step 1: read the file, detect a mapping ──
  async function pickFile(f) {
    if (!f) return;
    busy = true;
    error = '';
    try {
      file = { name: f.name, content: await fileToBase64(f) };
      dest = blankDest();
      // A file named BTCUSDT_1h.csv is naming the series; it is a suggestion in a field
      // the user can overwrite, never a value written behind their back.
      dest.label = f.name.replace(/\.[^.]+$/, '').slice(0, 80);
      await detectInto({});
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
    const a = await histdataApi.importAnalyze({
      filename: file.name,
      content: file.content,
      destination: dest,
      section
    });
    analysis = a;
    detected = Object.fromEntries(
      a.columns.map((c) => [c.index, { target: c.target, confidence: c.confidence }])
    );
    // Read naive timestamps in the reader's own zone. A bar is a period, so this decides
    // which period every row of the file belongs to, not merely how it is displayed.
    mapping = { ...a.mapping, tz_offset: -new Date().getTimezoneOffset() };
    // The file knows its own spacing; take it while the field is still empty.
    if (!dest.timeframe && a.destination?.timeframe) dest.timeframe = a.destination.timeframe;
  }

  // ── Re-preview on every edit (debounced) ──
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
      const a = await histdataApi.importAnalyze({
        filename: file.name,
        content: file.content,
        destination: dest,
        mapping
      });
      analysis = a;
      if (!dest.timeframe && a.destination?.timeframe) dest.timeframe = a.destination.timeframe;
    } catch (e) {
      error = e.message;
    } finally {
      previewing = false;
    }
  }

  // ── Column mapping ──
  const targets = $derived(
    IMPORT_TARGETS.map((t0) => ({ id: t0.id, label: $t(`histdata.import.field.${t0.key}`) }))
  );

  const columnLabels = $derived({
    ignore: $t('histdata.import.target.ignore'),
    noSamples: $t('histdata.import.columns.noSamples'),
    ambiguousDates: $t('histdata.import.ambiguousDates'),
    unnamed: (n) => $t('histdata.import.columns.unnamed', { n }),
    kind: (k) => $t(`histdata.import.kind.${k}`),
    confidence: (c) => $t(`histdata.import.confidence.${c}`)
  });
  const formatLabels = $derived({
    section: $t('histdata.import.section'),
    sectionFlat: $t('histdata.import.section.flat'),
    dateOrder: $t('histdata.import.dateOrder'),
    dateOrderAuto: $t('histdata.import.dateOrder.auto'),
    dateOrderDmy: $t('histdata.import.dateOrder.dmy'),
    dateOrderMdy: $t('histdata.import.dateOrder.mdy'),
    dateOrderYmd: $t('histdata.import.dateOrder.ymd'),
    decimal: $t('histdata.import.decimal'),
    decimalAuto: $t('histdata.import.decimal.auto'),
    decimalDot: $t('histdata.import.decimal.dot'),
    decimalComma: $t('histdata.import.decimal.comma'),
    timezone: $t('histdata.import.timezone')
  });

  const tsUnitOptions = $derived([
    { value: 'auto', label: $t('histdata.import.tsUnit.auto') },
    { value: 'text', label: $t('histdata.import.tsUnit.text') },
    { value: 's', label: $t('histdata.import.tsUnit.s') },
    { value: 'ms', label: $t('histdata.import.tsUnit.ms') },
    { value: 'us', label: $t('histdata.import.tsUnit.us') },
    { value: 'ns', label: $t('histdata.import.tsUnit.ns') }
  ]);

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
    // The bar fields are one-to-one: taking one frees whoever held it.
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

  /**
   * Reading another table of an export means other columns entirely: drop the mapping
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

  // ── Commit ──
  const stats = $derived(analysis?.stats ?? null);
  const missing = $derived.by(() => {
    const out = [];
    if (!dest.ticker.trim()) out.push($t('histdata.import.dest.ticker'));
    if (!dest.asset_type.trim()) out.push($t('histdata.import.dest.assetType'));
    if (!dest.timeframe.trim()) out.push($t('histdata.import.dest.timeframe'));
    return out;
  });
  const canImport = $derived(!busy && missing.length === 0 && (stats?.bars ?? 0) > 0);

  async function commit() {
    tagInput?.flush();
    busy = true;
    error = '';
    try {
      report = await histdataApi.importCommit({
        filename: file.name,
        content: file.content,
        mapping,
        destination: dest
      });
      step = 'done';
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
    dest = blankDest();
    error = '';
  }

  function close() {
    open = false;
    reset();
  }

  // Bar timestamps are read in UTC on purpose, like the dataset manager's ranges: a bar's
  // date is the exchange's date and must not shift with the viewer's timezone.
  const fmtTs = (iso) => (iso ? iso.replace('T', ' ').replace(/(:\d\d)(\.\d+)?Z?$/, '$1') : '—');
  const fmtDay = (iso) => (iso ? iso.slice(0, 10) : '—');
  const fmtPrice = (n) => (n === null || n === undefined ? '—' : fmtNum(n, 6));
</script>

<Modal bind:open size="xl" title={$t('histdata.import.title')} onclose={reset}>
  {#if step === 'pick'}
    <div class="pick">
      <FileDrop
        title={$t('histdata.import.pick.drop')}
        hint={$t('histdata.import.pick.hint')}
        browseLabel={$t('histdata.import.pick.browse')}
        busyLabel={$t('histdata.import.analyzing')}
        {busy}
        onpick={pickFile}
      />
      {#if error}
        <p class="error"><Icon name="alert-triangle" size={14} /> {error}</p>
      {/if}
    </div>
  {:else if step === 'map' && analysis && mapping}
    <div class="map">
      <!-- What the bars are. The file cannot answer any of this. -->
      <section class="dest">
        <h4>{$t('histdata.import.dest.title')}</h4>
        <div class="dest-grid">
          <Input
            label={$t('histdata.import.dest.name')}
            bind:value={dest.label}
            placeholder={$t('histdata.import.dest.namePlaceholder')}
            oninput={queue}
          />
          <Input
            label={$t('histdata.import.dest.ticker')}
            bind:value={dest.ticker}
            placeholder="BTCUSDT"
            required
            oninput={queue}
          />
          <Input
            label={$t('histdata.import.dest.assetType')}
            bind:value={dest.asset_type}
            list="otw-import-assets"
            placeholder="equity"
            required
            oninput={queue}
          />
          <Input
            label={$t('histdata.import.dest.timeframe')}
            bind:value={dest.timeframe}
            list="otw-import-timeframes"
            placeholder="1d"
            hint={analysis.stats.detected_timeframe
              ? $t('histdata.import.dest.timeframeDetected', {
                  tf: analysis.stats.detected_timeframe
                })
              : ''}
            required
            oninput={queue}
          />
          <Input
            label={$t('histdata.import.dest.source')}
            bind:value={dest.source}
            list="otw-import-sources"
            placeholder={$t('histdata.import.dest.sourcePlaceholder')}
            hint={$t('histdata.import.dest.sourceHint')}
            oninput={queue}
          />
          <div class="tags">
            <span class="lbl">{$t('histdata.import.dest.tags')}</span>
            <TagInput
              bind:this={tagInput}
              bind:tags={dest.tags}
              suggestions={tagSuggestions}
              listId="otw-import-tags"
              placeholder={$t('histdata.import.dest.tagsPlaceholder')}
            />
          </div>
        </div>
        <datalist id="otw-import-assets">
          {#each assetSuggestions as a (a)}<option value={a}></option>{/each}
        </datalist>
        <datalist id="otw-import-timeframes">
          {#each TIMEFRAMES as tf (tf)}<option value={tf}></option>{/each}
        </datalist>
        <datalist id="otw-import-sources">
          {#each sourceSuggestions as s (s)}<option value={s}></option>{/each}
        </datalist>
      </section>

      <!-- How to read the file. -->
      <section class="formats">
        <FormatBar
          {mapping}
          sections={analysis.sections}
          labels={formatLabels}
          onsection={setSection}
          onchange={set}
        />
        <div class="set">
          <span>{$t('histdata.import.tsUnit')}</span>
          <Dropdown
            value={mapping.ts_unit}
            onpick={(v) => set({ ts_unit: v })}
            ariaLabel={$t('histdata.import.tsUnit')}
            options={tsUnitOptions}
          />
        </div>
      </section>

      <section class="cols">
        <h4>
          {$t('histdata.import.columns.title')}
          <span class="sub"
            >{$t('histdata.import.columns.count', {
              cols: analysis.stats.columns,
              rows: fmtNum(analysis.stats.rows, 0)
            })}</span
          >
        </h4>
        <ColumnList
          columns={analysis.columns}
          {targets}
          labels={columnLabels}
          value={selectValue}
          confidence={confidenceOf}
          onselect={setTarget}
        />
        {#if analysis.synthesized.length}
          <p class="note">
            <Icon name="info" size={12} />
            {$t('histdata.import.synthesized', { fields: analysis.synthesized.join(', ') })}
          </p>
        {/if}
      </section>

      <!-- What the commit would write, over the whole file. -->
      <section class="stats">
        <div class="stat"><b>{fmtNum(stats.bars, 0)}</b><span>{$t('histdata.import.stats.bars')}</span></div>
        <div class="stat">
          <b>{fmtDay(stats.first_ts)} → {fmtDay(stats.last_ts)}</b>
          <span>{$t('histdata.import.stats.range')}</span>
        </div>
        {#if stats.missing_periods}
          <div class="stat warn">
            <b>{fmtNum(stats.missing_periods, 0)}</b>
            <span>{$t('histdata.import.stats.missing')}</span>
          </div>
        {/if}
        {#if stats.file_duplicates}
          <div class="stat warn">
            <b>{fmtNum(stats.file_duplicates, 0)}</b>
            <span>{$t('histdata.import.stats.fileDuplicates')}</span>
          </div>
        {/if}
        {#if stats.existing}
          <div class="stat warn">
            <b>{fmtNum(stats.existing, 0)}</b>
            <span>{$t('histdata.import.stats.existing')}</span>
          </div>
        {/if}
        {#if stats.inconsistent}
          <div class="stat warn">
            <b>{fmtNum(stats.inconsistent, 0)}</b>
            <span>{$t('histdata.import.stats.inconsistent')}</span>
          </div>
        {/if}
        {#if stats.errors}
          <div class="stat bad">
            <b>{fmtNum(stats.errors, 0)}</b>
            <span>{$t('histdata.import.stats.errors')}</span>
          </div>
        {/if}
        {#if previewing}<span class="busy">{$t('histdata.import.previewing')}</span>{/if}
      </section>

      {#if analysis.existing_dataset}
        <p class="note">
          <Icon name="info" size={12} />
          {$t('histdata.import.existingDataset', {
            bars: fmtNum(analysis.existing_dataset.bar_count, 0),
            from: fmtDay(analysis.existing_dataset.range_from),
            to: fmtDay(analysis.existing_dataset.range_to)
          })}
        </p>
      {/if}

      {#if analysis.preview.length}
        <section class="bars">
          <h4>{$t('histdata.import.preview.title')}</h4>
          <div class="scroller">
            <table class="tbl">
              <thead>
                <tr>
                  <th>{$t('histdata.import.preview.ts')}</th>
                  <th class="num">{$t('histdata.import.field.open')}</th>
                  <th class="num">{$t('histdata.import.field.high')}</th>
                  <th class="num">{$t('histdata.import.field.low')}</th>
                  <th class="num">{$t('histdata.import.field.close')}</th>
                  <th class="num">{$t('histdata.import.field.volume')}</th>
                  <th></th>
                </tr>
              </thead>
              <tbody>
                {#each analysis.preview as b (b.source_row)}
                  <tr class:odd={b.inconsistent}>
                    <td class="mono">{fmtTs(b.ts)}</td>
                    <td class="num mono">{fmtPrice(b.open)}</td>
                    <td class="num mono">{fmtPrice(b.high)}</td>
                    <td class="num mono">{fmtPrice(b.low)}</td>
                    <td class="num mono">{fmtPrice(b.close)}</td>
                    <td class="num mono">{fmtPrice(b.volume)}</td>
                    <td class="flags">
                      {#if b.existing}<span class="flag">{$t('histdata.import.flag.existing')}</span>{/if}
                      {#if b.inconsistent}<span class="flag warn"
                          >{$t('histdata.import.flag.inconsistent')}</span
                        >{/if}
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        </section>
      {/if}

      {#if analysis.errors.length}
        <section class="errs">
          <h4>{$t('histdata.import.errors.title')}</h4>
          <ul>
            {#each analysis.errors.slice(0, 30) as e (e.row + e.message)}
              <li><span class="ln">{$t('histdata.import.errors.line', { n: e.row })}</span> {e.message}</li>
            {/each}
          </ul>
          {#if analysis.errors.length > 30}
            <p class="note">{$t('histdata.import.errors.more', { n: analysis.errors.length - 30 })}</p>
          {/if}
        </section>
      {/if}

      {#if error}
        <p class="error"><Icon name="alert-triangle" size={14} /> {error}</p>
      {/if}

      <div class="actions">
        {#if missing.length}
          <span class="blocked">{$t('histdata.import.blocked', { fields: missing.join(', ') })}</span>
        {/if}
        <Button disabled={busy} onclick={() => reset()}>{$t('common.back')}</Button>
        <!-- The whole file is written in one request; the button carries the wait. -->
        <Button variant="primary" loading={busy} disabled={!canImport} onclick={commit}>
          {busy
            ? $t('histdata.import.importing')
            : $t('histdata.import.commit', { n: fmtNum(stats?.bars ?? 0, 0) })}
        </Button>
      </div>
    </div>
  {:else if step === 'done' && report}
    <div class="done">
      <p class="ok"><Icon name="check" size={16} /> {$t('histdata.import.done.title')}</p>
      <dl>
        <dt>{$t('histdata.import.done.dataset')}</dt>
        <dd class="mono">
          {report.ticker} · {report.timeframe} · {report.asset_type}{report.source
            ? ` · ${report.source}`
            : ''}
        </dd>
        <dt>{$t('histdata.import.done.written')}</dt>
        <dd>
          {$t('histdata.import.done.writtenValue', {
            inserted: fmtNum(report.inserted, 0),
            updated: fmtNum(report.updated, 0)
          })}
        </dd>
        <dt>{$t('histdata.import.done.range')}</dt>
        <dd class="mono">{fmtDay(report.range_from)} → {fmtDay(report.range_to)}</dd>
        <dt>{$t('histdata.import.done.total')}</dt>
        <dd>{$t('histdata.datasets.barsCount', { count: fmtNum(report.bar_count, 0) })}</dd>
      </dl>
      <div class="actions">
        <Button onclick={reset}>{$t('histdata.import.done.another')}</Button>
        <Button variant="primary" onclick={close}>{$t('common.close')}</Button>
      </div>
    </div>
  {/if}
</Modal>

<style>
  .map {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  h4 {
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--dim);
    margin-bottom: var(--space-2);
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
  }
  h4 .sub {
    text-transform: none;
    letter-spacing: 0;
    color: var(--muted);
  }
  .dest-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(170px, 1fr));
    gap: var(--space-3);
  }
  .tags {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }
  .tags .lbl {
    color: var(--muted);
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
  }
  .formats {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: var(--space-3);
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
  .stats {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-4);
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
  }
  .stat {
    display: flex;
    flex-direction: column;
  }
  .stat b {
    font-variant-numeric: tabular-nums;
  }
  .stat span {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .stat.warn b {
    color: var(--amber);
  }
  .stat.bad b {
    color: var(--red);
  }
  .busy {
    margin-left: auto;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .scroller {
    max-height: 34vh;
    overflow: auto;
    border: 0.5px solid var(--border);
  }
  .tbl {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }
  .tbl th,
  .tbl td {
    padding: 3px var(--space-2);
    text-align: left;
    border-bottom: 0.5px solid var(--border);
    white-space: nowrap;
  }
  .tbl thead th {
    position: sticky;
    top: 0;
    background: var(--surface);
    color: var(--muted);
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
  }
  .tbl .num {
    text-align: right;
  }
  .tbl tr.odd td {
    background: color-mix(in srgb, var(--amber) 8%, transparent);
  }
  .mono {
    font-family: var(--mono);
  }
  .flags {
    display: flex;
    gap: var(--space-1);
  }
  .flag {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
  }
  .flag.warn {
    color: var(--amber);
  }
  .errs ul {
    list-style: none;
    max-height: 22vh;
    overflow: auto;
    font-size: var(--text-sm);
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .errs .ln {
    color: var(--dim);
    font-family: var(--mono);
    margin-right: var(--space-2);
  }
  .note {
    font-size: var(--text-sm);
    color: var(--muted);
    display: flex;
    align-items: center;
    gap: var(--space-1);
    margin-top: var(--space-2);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    align-items: center;
    gap: var(--space-2);
  }
  .blocked {
    margin-right: auto;
    font-size: var(--text-sm);
    color: var(--amber);
  }
  .error {
    color: var(--red);
    font-size: var(--text-sm);
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }
  .pick {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .done {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .done .ok {
    color: var(--green);
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .done dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: var(--space-2) var(--space-4);
    font-size: var(--text-sm);
  }
  .done dt {
    color: var(--muted);
  }
</style>
