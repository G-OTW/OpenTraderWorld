<script>
  // Gap check of one stored dataset: where the market traded and no candle is stored, sorted
  // by how likely a download is to find it, with what asking the provider again would cost.
  //
  // The server learns the sessions from the series itself (nights, weekends and the daily
  // break of a future are never holes), so this view only shows and chooses. Filling queues
  // ordinary download jobs; a hole the provider has nothing for comes back `confirmed` on the
  // next check and is never asked for twice.
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { histdataApi } from './api.js';
  import { t } from '$lib/i18n';
  import { fmtNum } from '$lib/format.js';

  let { open = $bindable(false), dataset = null, onfilled } = $props();

  const KINDS = ['missing', 'holiday', 'quiet'];

  let report = $state(null);
  let error = $state('');
  let loading = $state(false);
  let filling = $state(false);
  let queued = $state(null);
  // Chosen kinds. What is unlikely to exist at the provider starts unticked.
  let chosen = $state({ missing: true, holiday: false, quiet: false });
  let largestFirst = $state(false);

  $effect(() => {
    if (open && dataset) load(dataset.id);
  });

  async function load(id) {
    loading = true;
    error = '';
    report = null;
    queued = null;
    chosen = { missing: true, holiday: false, quiet: false };
    try {
      report = await histdataApi.checkDataset(id);
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  const kindOf = (k) => report?.kinds?.[k] ?? { holes: 0, bars: 0, open: 0, open_bars: 0, requests: 0 };
  const canFill = $derived(!!report?.fill?.available);
  const selected = $derived(KINDS.filter((k) => chosen[k] && kindOf(k).open > 0));
  // Each kind is estimated on its own; two kinds together can only share pages, so the sum
  // is an upper bound.
  const requests = $derived(selected.reduce((n, k) => n + (kindOf(k).requests ?? 0), 0));
  const openHoles = $derived(selected.reduce((n, k) => n + kindOf(k).open, 0));
  const seconds = $derived(Math.round((requests * (report?.fill?.min_interval_ms ?? 0)) / 1000));
  const quotaMax = $derived(report?.fill?.quota_max ?? null);
  const quotaLeft = $derived(quotaMax == null ? null : Math.max(0, quotaMax - (report?.fill?.quota_used ?? 0)));
  const overQuota = $derived(quotaLeft != null && requests > quotaLeft);

  const coverage = $derived(report && report.expected > 0 ? (report.bars / report.expected) * 100 : 100);
  const clean = $derived(report && report.total_holes === 0);

  // The strip: the dataset's whole span, one mark per hole (at least a hairline wide).
  const span = $derived.by(() => {
    const f = Date.parse(report?.dataset?.range_from ?? '');
    const l = Date.parse(report?.dataset?.range_to ?? '');
    return Number.isFinite(f) && Number.isFinite(l) && l > f ? { f, l } : null;
  });
  const marks = $derived.by(() => {
    if (!span || !report) return [];
    const w = span.l - span.f;
    return report.holes.map((h) => {
      const a = Date.parse(h.from);
      const b = Date.parse(h.to);
      return { ...h, left: ((a - span.f) / w) * 100, width: ((b - a) / w) * 100 };
    });
  });

  const rows = $derived(
    report ? (largestFirst ? [...report.holes].sort((a, b) => b.bars - a.bars) : report.holes) : []
  );

  const daily = $derived((report?.step_secs ?? 0) >= 86400);
  // UTC on purpose: a candle's date is the exchange's, not the viewer's.
  const stamp = (s) => {
    const iso = new Date(s).toISOString();
    return daily ? iso.slice(0, 10) : `${iso.slice(0, 10)} ${iso.slice(11, 16)}`;
  };
  function human(secs) {
    if (secs < 60) return $t('histdata.check.dur.sec', { n: Math.max(1, secs) });
    if (secs < 3600) return $t('histdata.check.dur.min', { n: Math.round(secs / 60) });
    if (secs < 86400 * 2) return $t('histdata.check.dur.hour', { n: fmtNum(secs / 3600, 1) });
    return $t('histdata.check.dur.day', { n: fmtNum(secs / 86400, 1) });
  }
  const holeSpan = (h) => human(Math.round((Date.parse(h.to) - Date.parse(h.from)) / 1000));

  async function fill() {
    if (!report || !selected.length || filling) return;
    filling = true;
    error = '';
    try {
      queued = await histdataApi.fillGaps(report.dataset.id, selected);
      onfilled?.();
    } catch (e) {
      error = e.message;
    } finally {
      filling = false;
    }
  }

  const title = $derived(
    dataset ? $t('histdata.check.title', { ticker: dataset.label || dataset.ticker, timeframe: dataset.timeframe }) : ''
  );
</script>

<Modal bind:open {title} size="lg">
  {#if loading}
    <div class="state" aria-live="polite">
      <span class="spin" aria-hidden="true"></span>
      <p>{$t('histdata.check.scanning', { count: fmtNum(dataset?.bar_count ?? 0, 0) })}</p>
    </div>
  {:else if error && !report}
    <p class="err">{error}</p>
  {:else if queued}
    <div class="state done" aria-live="polite">
      <span class="badge ok"><Icon name="check" size={20} /></span>
      {#if queued.jobs}
        <p class="lead">{$t('histdata.check.queued', { jobs: queued.jobs, holes: fmtNum(queued.holes, 0) })}</p>
        <p class="sub">{$t('histdata.check.queuedHint', { requests: fmtNum(queued.requests, 0) })}</p>
      {:else}
        <p class="lead">{$t('histdata.check.queuedNothing')}</p>
      {/if}
    </div>
  {:else if report}
    <div class="head">
      <div class="cov" class:good={coverage >= 99.95} class:warn={coverage < 99.95 && coverage >= 98} class:bad={coverage < 98}>
        <span class="pct">{fmtNum(Math.floor(coverage * 100) / 100, 2)}%</span>
        <span class="cap">{$t('histdata.check.coverage')}</span>
      </div>
      <div class="facts">
        <p>
          {$t('histdata.check.barsOf', { bars: fmtNum(report.bars, 0), expected: fmtNum(report.expected, 0) })}
        </p>
        <p class="muted">
          <span class="src">{report.dataset.provider}</span>
          {#if span}· {stamp(report.dataset.range_from)} → {stamp(report.dataset.range_to)}{/if}
        </p>
        <p class="cal">
          <Icon name={report.around_the_clock ? 'clock' : 'calendar-days'} size={13} />
          {report.around_the_clock ? $t('histdata.check.calendar24') : $t('histdata.check.calendarSessions')}
        </p>
      </div>
    </div>

    {#if span}
      <div class="strip" role="img" aria-label={$t('histdata.check.stripLabel', { n: report.total_holes })}>
        {#each marks as m, i (i)}
          <span
            class="mark"
            data-kind={m.kind}
            data-status={m.status}
            style="left: {m.left}%; width: max(2px, {m.width}%)"
            title="{stamp(m.from)} → {stamp(m.to)} · {$t('histdata.check.missingBars', { n: fmtNum(m.bars, 0) })}"
          ></span>
        {/each}
      </div>
      <div class="axis"><span>{stamp(report.dataset.range_from)}</span><span>{stamp(report.dataset.range_to)}</span></div>
    {/if}

    {#if clean}
      <div class="state done">
        <span class="badge ok"><Icon name="check" size={20} /></span>
        <p class="lead">{$t('histdata.check.clean')}</p>
        <p class="sub">{$t('histdata.check.cleanHint')}</p>
      </div>
    {:else}
      <div class="kinds" role="group" aria-label={$t('histdata.check.chooseLabel')}>
        {#each KINDS as k (k)}
          {@const s = kindOf(k)}
          {#if s.holes > 0}
            <label class="kind" data-kind={k} class:off={!s.open || !canFill} class:on={chosen[k] && s.open > 0 && canFill}>
              <!-- Nothing left to ask for reads unticked, whatever was chosen. -->
              <input
                type="checkbox"
                checked={chosen[k] && s.open > 0 && canFill}
                disabled={!s.open || !canFill}
                onchange={(e) => (chosen[k] = e.currentTarget.checked)}
              />
              <span class="dot" aria-hidden="true"></span>
              <span class="body">
                <span class="name">{$t(`histdata.check.kind.${k}`)}</span>
                <span class="desc">{$t(`histdata.check.kindHint.${k}`)}</span>
              </span>
              <span class="nums">
                <span class="big">{fmtNum(s.bars, 0)}</span>
                <span class="small">{$t('histdata.check.inHoles', { n: fmtNum(s.holes, 0) })}</span>
                {#if s.open && canFill}
                  <span class="small req">≈ {$t('histdata.check.requests', { n: fmtNum(s.requests ?? 0, 0) })}</span>
                {:else if !s.open}
                  <span class="small">{$t('histdata.check.nothingOpen')}</span>
                {/if}
              </span>
            </label>
          {/if}
        {/each}
      </div>

      {#if report.filling || report.confirmed}
        <ul class="notes">
          {#if report.filling}<li data-status="filling">{$t('histdata.check.filling', { n: report.filling })}</li>{/if}
          {#if report.confirmed}<li data-status="confirmed">{$t('histdata.check.confirmed', { n: report.confirmed })}</li>{/if}
        </ul>
      {/if}

      {#if !canFill}
        <p class="unavail"><Icon name="info" size={13} /> {report.fill?.reason}</p>
      {/if}

      <details class="list">
        <summary>
          {$t('histdata.check.showHoles', { n: fmtNum(report.total_holes, 0) })}
          {#if report.total_holes > report.holes.length}
            <span class="muted">· {$t('histdata.check.listedFirst', { n: fmtNum(report.holes.length, 0) })}</span>
          {/if}
        </summary>
        <div class="list-bar">
          <label class="sort"><input type="checkbox" bind:checked={largestFirst} /> {$t('histdata.check.largestFirst')}</label>
        </div>
        <div class="table">
          <div class="tr th">
            <span>{$t('histdata.check.col.from')}</span>
            <span>{$t('histdata.check.col.to')}</span>
            <span class="r">{$t('histdata.check.col.bars')}</span>
            <span class="r">{$t('histdata.check.col.span')}</span>
            <span>{$t('histdata.check.col.kind')}</span>
            <span>{$t('histdata.check.col.status')}</span>
          </div>
          {#each rows as h, i (i)}
            <div class="tr">
              <span class="mono">{stamp(h.from)}</span>
              <span class="mono">{stamp(h.to)}</span>
              <span class="r mono">{fmtNum(h.bars, 0)}</span>
              <span class="r muted">{holeSpan(h)}</span>
              <span><span class="chip" data-kind={h.kind}>{$t(`histdata.check.kind.${h.kind}`)}</span></span>
              <span class="status" data-status={h.status}>{$t(`histdata.check.status.${h.status}`)}</span>
            </div>
          {/each}
        </div>
      </details>
    {/if}
    {#if error}<p class="err">{error}</p>{/if}
  {/if}

  {#snippet footer()}
    {#if report && !queued && !clean && canFill}
      <div class="cost" aria-live="polite">
        {#if selected.length}
          <span class="line">
            {$t('histdata.check.estimate', { requests: fmtNum(requests, 0) })}
            {#if seconds > 0}<span class="muted">· {$t('histdata.check.atLeast', { time: human(seconds) })}</span>{/if}
          </span>
          {#if quotaMax != null}
            <span class="line small" class:warn={overQuota}>
              {#if overQuota}<Icon name="alert-triangle" size={12} />{/if}
              {overQuota
                ? $t('histdata.check.overQuota', { left: fmtNum(quotaLeft, 0), max: fmtNum(quotaMax, 0), period: report.fill.quota_period })
                : $t('histdata.check.quota', { left: fmtNum(quotaLeft, 0), max: fmtNum(quotaMax, 0), period: report.fill.quota_period })}
            </span>
          {/if}
          <span class="line small muted">{$t('histdata.check.estimateHint')}</span>
        {:else}
          <span class="line muted">{$t('histdata.check.noneChosen')}</span>
        {/if}
      </div>
      <Button onclick={() => (open = false)}>{$t('histdata.check.keep')}</Button>
      <Button variant="primary" icon="download" loading={filling} disabled={!selected.length} onclick={fill}>
        {openHoles ? $t('histdata.check.fill', { n: fmtNum(openHoles, 0) }) : $t('histdata.check.fillNone')}
      </Button>
    {:else}
      <Button onclick={() => (open = false)}>{$t('common.close')}</Button>
    {/if}
  {/snippet}
</Modal>

<style>
  .state {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-6) var(--space-4);
    text-align: center;
    color: var(--muted);
  }
  .state.done {
    color: var(--text);
  }
  .lead {
    font-weight: var(--fw-medium);
    font-size: var(--text-md);
  }
  .sub {
    color: var(--muted);
    font-size: var(--text-sm);
    max-width: 52ch;
  }
  .badge {
    display: inline-grid;
    place-items: center;
    width: 40px;
    height: 40px;
    border-radius: 50%;
  }
  .badge.ok {
    color: var(--green);
    background: color-mix(in srgb, var(--green) 14%, transparent);
  }
  .spin {
    width: 22px;
    height: 22px;
    border: 2px solid var(--accent);
    border-top-color: transparent;
    border-radius: 50%;
    animation: spin 700ms linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .head {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    margin-bottom: var(--space-4);
  }
  .cov {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    min-width: 112px;
    padding: var(--space-3);
    border-radius: var(--radius-lg);
    background: var(--surface-2);
  }
  .cov .pct {
    font-size: var(--text-xl);
    font-weight: var(--fw-semibold);
    font-variant-numeric: tabular-nums;
  }
  .cov .cap {
    font-size: var(--text-xs);
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .cov.good .pct {
    color: var(--green);
  }
  .cov.warn .pct {
    color: var(--amber);
  }
  .cov.bad .pct {
    color: var(--red);
  }
  .facts {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }
  .facts .src {
    text-transform: capitalize;
  }
  .cal {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .muted {
    color: var(--muted);
  }

  .strip {
    position: relative;
    height: 22px;
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--green) 22%, var(--surface-2));
    overflow: hidden;
  }
  .mark {
    position: absolute;
    top: 0;
    bottom: 0;
    background: var(--red);
  }
  .mark[data-kind='holiday'] {
    background: var(--amber);
  }
  .mark[data-kind='quiet'] {
    background: var(--muted);
    top: 35%;
    bottom: 35%;
  }
  .mark[data-status='confirmed'] {
    opacity: 0.35;
  }
  .mark[data-status='filling'] {
    background: var(--accent);
  }
  .axis {
    display: flex;
    justify-content: space-between;
    font-size: var(--text-xs);
    color: var(--muted);
    margin: var(--space-1) 0 var(--space-4);
    font-variant-numeric: tabular-nums;
  }

  .kinds {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .kind {
    display: grid;
    grid-template-columns: auto auto 1fr auto;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    cursor: pointer;
    transition: border-color 120ms, background 120ms;
  }
  .kind.on {
    border-color: var(--accent);
    background: var(--accent-soft);
  }
  .kind.off {
    cursor: default;
    opacity: 0.7;
  }
  .kind input {
    margin: 0;
  }
  .dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--red);
  }
  .kind[data-kind='holiday'] .dot {
    background: var(--amber);
  }
  .kind[data-kind='quiet'] .dot {
    background: var(--muted);
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .name {
    font-weight: var(--fw-medium);
  }
  .desc {
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .nums {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .big {
    font-size: var(--text-lg);
    font-weight: var(--fw-semibold);
  }
  .small {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .small.req {
    color: var(--text);
  }

  .notes {
    list-style: none;
    margin: var(--space-3) 0 0;
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-4);
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .notes li::before {
    content: '';
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    margin-right: var(--space-1);
    background: var(--accent);
  }
  .notes li[data-status='confirmed']::before {
    background: var(--muted);
  }
  .unavail {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    margin-top: var(--space-3);
    font-size: var(--text-sm);
    color: var(--amber);
  }

  .list {
    margin-top: var(--space-4);
  }
  .list summary {
    cursor: pointer;
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
  }
  .list-bar {
    display: flex;
    justify-content: flex-end;
    margin: var(--space-2) 0;
  }
  .sort {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .table {
    max-height: 280px;
    overflow: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    font-size: var(--text-sm);
  }
  .tr {
    display: grid;
    grid-template-columns: 1fr 1fr 80px 72px 120px 130px;
    gap: var(--space-2);
    align-items: center;
    padding: var(--space-1) var(--space-3);
  }
  .tr + .tr {
    border-top: 1px solid var(--border);
  }
  .th {
    position: sticky;
    top: 0;
    background: var(--surface);
    color: var(--muted);
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .r {
    text-align: right;
  }
  .mono {
    font-variant-numeric: tabular-nums;
  }
  .chip {
    display: inline-block;
    padding: 0 6px;
    border-radius: var(--radius);
    font-size: var(--text-xs);
    color: var(--red);
    background: color-mix(in srgb, var(--red) 12%, transparent);
  }
  .chip[data-kind='holiday'] {
    color: var(--amber);
    background: color-mix(in srgb, var(--amber) 14%, transparent);
  }
  .chip[data-kind='quiet'] {
    color: var(--muted);
    background: var(--surface-2);
  }
  .status {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .status[data-status='filling'] {
    color: var(--accent);
  }

  .cost {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-right: auto;
    min-width: 0;
  }
  .cost .line {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    flex-wrap: wrap;
  }
  .cost .warn {
    color: var(--amber);
  }
  .err {
    color: var(--red);
    font-size: var(--text-sm);
    margin-top: var(--space-3);
  }

  @media (max-width: 767px) {
    .head {
      flex-direction: column;
      align-items: stretch;
    }
    .kind {
      grid-template-columns: auto auto 1fr;
    }
    .kind .nums {
      grid-column: 3;
      align-items: flex-start;
      text-align: left;
    }
    .tr {
      grid-template-columns: 1fr 1fr 64px;
    }
    .tr > :nth-child(n + 4) {
      display: none;
    }
  }
</style>
