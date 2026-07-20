<script>
  // Journal export dialog: raw trades as CSV, or the weekly/monthly performance
  // report rendered server-side to Markdown/PDF. Scoped to the currently
  // selected category (or all categories).
  import Modal from '$lib/ui/Modal.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { t, locale } from '$lib/i18n';
  import { journalApi } from './api.js';

  let { open = $bindable(false), categoryId = '', categoryName = '' } = $props();

  let kind = $state('report'); // 'csv' | 'report'
  let range = $state('week'); // csv: 'all' | 'week' | 'month' — report: 'week' | 'month'
  let format = $state('pdf'); // report only: 'pdf' | 'md'
  // Anchor: any date inside the selected week/month (navigated with ‹ ›).
  let anchor = $state(new Date());
  let busy = $state(false);
  let error = $state('');

  // Reset transient state each time the dialog opens.
  $effect(() => {
    if (open) {
      anchor = new Date();
      error = '';
    }
  });

  // CSV can additionally export the full history.
  const ranges = $derived(kind === 'csv' ? ['all', 'week', 'month'] : ['week', 'month']);
  $effect(() => {
    if (!ranges.includes(range)) range = ranges[0];
  });

  // ── Period math (UTC, mirrors the backend's ISO week / calendar month) ──
  function weekBounds(d) {
    const day = new Date(Date.UTC(d.getFullYear(), d.getMonth(), d.getDate()));
    const dow = (day.getUTCDay() + 6) % 7; // Monday = 0
    const start = new Date(day);
    start.setUTCDate(day.getUTCDate() - dow);
    const end = new Date(start);
    end.setUTCDate(start.getUTCDate() + 7);
    return [start, end];
  }
  function monthBounds(d) {
    return [
      new Date(Date.UTC(d.getFullYear(), d.getMonth(), 1)),
      new Date(Date.UTC(d.getFullYear(), d.getMonth() + 1, 1))
    ];
  }
  const bounds = $derived(range === 'month' ? monthBounds(anchor) : weekBounds(anchor));

  function isoWeek(d) {
    // ISO-8601 week number of the Thursday in this date's week.
    const t0 = new Date(Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate()));
    t0.setUTCDate(t0.getUTCDate() + 3 - ((t0.getUTCDay() + 6) % 7));
    const jan4 = new Date(Date.UTC(t0.getUTCFullYear(), 0, 4));
    return 1 + Math.round(((t0 - jan4) / 86400000 - 3 + ((jan4.getUTCDay() + 6) % 7)) / 7);
  }

  const periodLabel = $derived.by(() => {
    if (range === 'all') return $t('journal.export.range.allHint');
    const [start, end] = bounds;
    if (range === 'month') {
      return start.toLocaleDateString($locale, {
        month: 'long',
        year: 'numeric',
        timeZone: 'UTC'
      });
    }
    const last = new Date(end - 86400000);
    const fmt = (x, opts) => x.toLocaleDateString($locale, { ...opts, timeZone: 'UTC' });
    return `${fmt(start, { month: 'short', day: 'numeric' })} – ${fmt(last, {
      month: 'short',
      day: 'numeric',
      year: 'numeric'
    })} · ${$t('journal.export.weekN', { n: isoWeek(start) })}`;
  });

  function step(dir) {
    const d = new Date(anchor);
    if (range === 'month') d.setMonth(d.getMonth() + dir);
    else d.setDate(d.getDate() + dir * 7);
    anchor = d;
  }
  // Don't navigate past the current period.
  const nextDisabled = $derived(range !== 'all' && bounds[1] > new Date());

  function isoDate(d) {
    return d.toISOString().slice(0, 10);
  }

  async function download() {
    busy = true;
    error = '';
    try {
      if (kind === 'csv') {
        const filter = { category_id: categoryId };
        if (range !== 'all') {
          const [start, end] = bounds;
          filter.since = start.toISOString();
          filter.until = new Date(end - 1000).toISOString();
        }
        await journalApi.exportTradesCsv(filter);
      } else {
        await journalApi.exportReport({
          period: range,
          anchor: isoDate(anchor),
          format,
          category_id: categoryId
        });
      }
      open = false;
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }
</script>

<Modal bind:open title={$t('journal.export.title')} size="md">
  <div class="export">
    <!-- What to export -->
    <div class="kinds">
      <button class="kind" class:active={kind === 'csv'} onclick={() => (kind = 'csv')}>
        <span class="kicon"><Icon name="file-text" size={18} /></span>
        <span class="ktext">
          <span class="kname">{$t('journal.export.kind.csv')}</span>
          <span class="kdesc">{$t('journal.export.kind.csvDesc')}</span>
        </span>
      </button>
      <button class="kind" class:active={kind === 'report'} onclick={() => (kind = 'report')}>
        <span class="kicon"><Icon name="bar-chart" size={18} /></span>
        <span class="ktext">
          <span class="kname">{$t('journal.export.kind.report')}</span>
          <span class="kdesc">{$t('journal.export.kind.reportDesc')}</span>
        </span>
      </button>
    </div>

    <!-- Scope -->
    <div class="row">
      <span class="label">{$t('journal.export.scope')}</span>
      <span class="badge">
        {categoryId ? categoryName : $t('journal.export.scopeAll')}
      </span>
    </div>

    <!-- Period -->
    <div class="row">
      <span class="label">{$t('journal.export.period')}</span>
      <div class="seg">
        {#each ranges as rid}
          <button class="seg-btn" class:active={range === rid} onclick={() => (range = rid)}>
            {$t(`journal.export.range.${rid}`)}
          </button>
        {/each}
      </div>
    </div>

    {#if range !== 'all'}
      <div class="row">
        <span class="label"></span>
        <div class="nav">
          <button
            class="icon"
            title={$t('journal.export.prevPeriod')}
            aria-label={$t('journal.export.prevPeriod')}
            onclick={() => step(-1)}><Icon name="chevron-left" size={16} /></button
          >
          <span class="period-label">{periodLabel}</span>
          <button
            class="icon"
            title={$t('journal.export.nextPeriod')}
            aria-label={$t('journal.export.nextPeriod')}
            disabled={nextDisabled}
            onclick={() => step(1)}><Icon name="chevron-right" size={16} /></button
          >
        </div>
      </div>
    {/if}

    <!-- Format (report only) -->
    {#if kind === 'report'}
      <div class="row">
        <span class="label">{$t('journal.export.format')}</span>
        <div class="seg">
          <button class="seg-btn" class:active={format === 'pdf'} onclick={() => (format = 'pdf')}>
            PDF
          </button>
          <button class="seg-btn" class:active={format === 'md'} onclick={() => (format = 'md')}>
            Markdown
          </button>
        </div>
      </div>
    {/if}

    {#if error}
      <p class="error"><Icon name="alert-triangle" size={14} /> {error}</p>
    {/if}

    <div class="actions">
      <button class="primary" disabled={busy} onclick={download}>
        <Icon name="download" size={16} />
        {busy ? $t('journal.export.preparing') : $t('journal.export.download')}
      </button>
    </div>
  </div>
</Modal>

<style>
  .export {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .kinds {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-3);
  }
  .kind {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    text-align: left;
    padding: var(--space-3);
    background: var(--bg);
    border: 0.5px solid var(--border);
    border-left: 1.5px solid transparent;
    border-radius: 0;
    color: var(--text);
    cursor: pointer;
    font: inherit;
    transition: background-color var(--dur-fast) var(--ease);
  }
  .kind:hover {
    background: var(--surface-2);
  }
  .kind.active {
    border-left-color: var(--accent);
    background: var(--surface-2);
  }
  .kicon {
    color: var(--dim);
    margin-top: 2px;
  }
  .kind.active .kicon {
    color: var(--accent);
  }
  .ktext {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .kname {
    font-weight: var(--fw-medium);
    font-size: var(--text-sm);
    letter-spacing: 0.02em;
  }
  .kdesc {
    color: var(--muted);
    font-size: var(--text-xs);
    line-height: 1.35;
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-height: 28px;
  }
  .label {
    width: 72px;
    flex-shrink: 0;
    color: var(--dim);
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .seg {
    display: inline-flex;
    border: 0.5px solid var(--border-control);
    border-radius: 0;
    overflow: hidden;
  }
  .seg-btn {
    background: transparent;
    border: none;
    color: var(--muted);
    padding: 6px 12px;
    font: inherit;
    font-size: var(--text-sm);
    cursor: pointer;
    transition: background-color var(--dur-fast) var(--ease);
  }
  .seg-btn + .seg-btn {
    border-left: 0.5px solid var(--border-control);
  }
  .seg-btn.active {
    background: var(--surface-2);
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  .nav {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
  }
  .period-label {
    min-width: 210px;
    text-align: center;
    font-family: var(--mono);
    font-size: var(--text-sm);
    font-variant-numeric: tabular-nums;
  }
  .error {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--red);
    font-size: var(--text-sm);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    padding-top: var(--space-2);
    border-top: 0.5px solid var(--border);
  }
</style>
