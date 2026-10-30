<script>
  // One what-if scenario (size, filter, reverse or fees) over the closed trades of the
  // current scope. The server replays them (/api/journal/lab/simulate) and decides whether
  // a statement fires; this panel holds the form, then shows the verdict, the before and
  // after table, the curves and every trade, with the trades it could not read named.
  import { journalApi, fmtSignedMoney, fmtMoney, fmtPct, fmtNum } from './api.js';
  import { buildScenario, formOf, countBy, statementParams, WEEKDAYS } from './lab.js';
  import { SEVERITY_ICON } from './insights.js';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import { t, locale } from '$lib/i18n';
  import Button from '$lib/ui/Button.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import SignalGroupEditor from '$lib/modules/backtest/SignalGroupEditor.svelte';
  import ExitCurves from './ExitCurves.svelte';
  import LabSummary from './LabSummary.svelte';
  import LabTradeTable from './LabTradeTable.svelte';

  // `initial` = a scenario to open with (from the strategy card), replacing the form.
  let { kind, categoryId = '', feeSchedules = [], families = [], minSample = 20, initial = null, ongofees = null } =
    $props();

  let form = $state(formOf(null));
  let lastInitial = null;
  $effect(() => {
    if (initial && initial !== lastInitial && initial.kind === kind) {
      lastInitial = initial;
      form = formOf(initial);
    }
  });

  let outcome = $state(null);
  let busy = $state(false);
  let err = $state('');
  const colors = $derived(chartColors());

  async function run() {
    const built = buildScenario(kind, $state.snapshot(form));
    if (built.error) {
      err = $t(built.error);
      return;
    }
    busy = true;
    err = '';
    try {
      const r = await journalApi.labSimulate(built.scenario, { category_id: categoryId || undefined });
      outcome = r.outcome;
    } catch (e) {
      err = e.message;
    } finally {
      busy = false;
    }
  }

  // ── Formatting ──
  const cur = $derived(outcome?.currency ?? 'USD');
  const F = $derived({
    money: (v) => fmtSignedMoney(v, cur),
    int: (v) => String(Math.round(v)),
    factor: (v) => `×${fmtNum(v, 2)}`,
    pct: (v) => fmtPct(v, 1),
    num: (v) => fmtNum(v, 2)
  });
  const statementText = $derived(
    outcome?.statement
      ? $t(`journal.lab.statement.${outcome.statement.key}`, statementParams(outcome.statement.values, F))
      : ''
  );
  const weekdayName = (d) =>
    new Date(Date.UTC(2024, 0, d)).toLocaleDateString($locale, { weekday: 'short', timeZone: 'UTC' });
  function toggleDay(d) {
    form.weekdays = form.weekdays.includes(d) ? form.weekdays.filter((x) => x !== d) : [...form.weekdays, d];
  }

  const NOTE = { capped: 'capped', removed: 'removed', flipped: 'flipped', partial: 'partial', too_small: 'tooSmall' };
  const rows = $derived(
    (outcome?.rows ?? []).map((r) => ({
      ...r,
      at: r.exit_at,
      outcomeLabel: r.note ? $t(`journal.lab.note.${NOTE[r.note] ?? r.note}`) : '',
      muted: !r.note || r.note === 'removed' || r.note === 'capped'
    }))
  );
  const unmeasured = $derived(countBy(outcome?.unmeasured ?? [], 'reason'));
  // Trades an indicator filter could not read for want of candles: the Market data strip
  // above the lab downloads them.
  const needCandles = $derived(unmeasured.some(([reason]) => reason === 'no_bars' || reason === 'thin'));
  const d = $derived(outcome?.detail);
  const afterLabel = $derived($t(`journal.lab.after.${kind}`));
</script>

<div class="panel">
  <p class="hint">{$t(`journal.lab.${kind}.hint`)}</p>

  <div class="form">
    {#if kind === 'size'}
      <label class="field">
        <span>{$t('journal.lab.size.factor')}</span>
        <input type="number" min="0.5" max="3" step="0.1" bind:value={form.factor} />
      </label>
      <label class="field">
        <span>{$t('journal.lab.size.cap')}</span>
        <input type="number" min="0" step="any" placeholder={$t('journal.lab.size.capPlaceholder')} bind:value={form.cap} />
      </label>
    {/if}

    {#if kind === 'reverse'}
      <div class="field">
        <span>{$t('journal.lab.reverse.which')}</span>
        <Dropdown
          value={form.onlyMatching ? 'only' : 'all'}
          onpick={(v) => (form.onlyMatching = v === 'only')}
          ariaLabel={$t('journal.lab.reverse.which')}
          options={[
            { value: 'all', label: $t('journal.lab.reverse.all') },
            { value: 'only', label: $t('journal.lab.reverse.only') }
          ]}
        />
      </div>
    {/if}

    {#if kind === 'filter' || (kind === 'reverse' && form.onlyMatching)}
      <div class="filter">
        <label class="check">
          <input type="checkbox" bind:checked={form.useWindow} />
          {$t('journal.lab.filter.window')}
        </label>
        {#if form.useWindow}
          <div class="row">
            <label class="field">
              <span>{$t('journal.lab.filter.hours')}</span>
              <input placeholder={$t('journal.lab.filter.hoursPlaceholder')} bind:value={form.hours} />
            </label>
            <div class="field">
              <span>{$t('journal.lab.filter.weekdays')}</span>
              <div class="days">
                {#each WEEKDAYS as wd}
                  <button type="button" class="day" class:on={form.weekdays.includes(wd)} onclick={() => toggleDay(wd)}>
                    {weekdayName(wd)}
                  </button>
                {/each}
              </div>
            </div>
            <div class="field">
              <span>{$t('journal.lab.filter.mode')}</span>
              <Dropdown
                value={form.exclude ? 'drop' : 'keep'}
                onpick={(v) => (form.exclude = v === 'drop')}
                ariaLabel={$t('journal.lab.filter.mode')}
                options={[
                  { value: 'drop', label: $t('journal.lab.filter.drop') },
                  { value: 'keep', label: $t('journal.lab.filter.keep') }
                ]}
              />
            </div>
          </div>
        {/if}
        <label class="check">
          <input type="checkbox" bind:checked={form.useSignal} />
          {$t('journal.lab.filter.signal')}
        </label>
        {#if form.useSignal}
          <p class="hint">{$t('journal.lab.filter.signalHint')}</p>
          <SignalGroupEditor
            bind:group={form.signal}
            title={$t('journal.lab.filter.signalTitle')}
            defaultOp="above"
            emptyHint={$t('journal.lab.filter.signalEmpty')}
          />
        {/if}
      </div>
    {/if}

    {#if kind === 'fees'}
      {#if !feeSchedules.length}
        <p class="warn">
          {$t('journal.lab.fees.noSchedule')}
          {#if ongofees}<button type="button" class="link" onclick={ongofees}>{$t('journal.lab.fees.addSchedule')}</button>{/if}
        </p>
      {:else}
        <div class="field">
          <span>{$t('journal.lab.fees.schedule')}</span>
          <Dropdown
            bind:value={form.scheduleId}
            placeholder={$t('journal.lab.fees.pickSchedule')}
            ariaLabel={$t('journal.lab.fees.schedule')}
            options={feeSchedules.map((s) => ({ value: s.id, label: s.name }))}
          />
        </div>
        <div class="field">
          <span>{$t('journal.lab.fees.family')}</span>
          <Dropdown
            bind:value={form.family}
            ariaLabel={$t('journal.lab.fees.family')}
            options={[
              { value: '', label: $t('journal.lab.fees.sameInstrument') },
              ...families.map((f) => ({ value: f.id, label: `${f.small} / ${f.large} (${f.ratio})` }))
            ]}
          />
        </div>
        {#if form.family}
          {@const fam = families.find((f) => f.id === form.family)}
          <div class="field">
            <span>{$t('journal.lab.fees.to')}</span>
            <Dropdown
              bind:value={form.to}
              ariaLabel={$t('journal.lab.fees.to')}
              options={[
                { value: 'large', label: $t('journal.lab.fees.toLarge', { from: fam?.small ?? '', to: fam?.large ?? '' }) },
                { value: 'small', label: $t('journal.lab.fees.toSmall', { from: fam?.large ?? '', to: fam?.small ?? '' }) }
              ]}
            />
          </div>
        {/if}
      {/if}
    {/if}

    <div class="actions">
      <Button variant="primary" icon="play" loading={busy} onclick={run} disabled={kind === 'fees' && !feeSchedules.length}>
        {$t('journal.lab.run')}
      </Button>
    </div>
  </div>

  <ErrorText error={err} />

  {#if busy && !outcome}
    <Skeleton rows={6} height="var(--row-h)" gap="1px" />
  {:else if outcome}
    {#if unmeasured.length}
      <p class="skipped">
        {$t('journal.lab.unmeasured', { n: outcome.unmeasured.length })}
        {#each unmeasured as [reason, n]}
          <span class="reason">{$t(`journal.exits.reason.${reason}`)} ({n})</span>
        {/each}
      </p>
      {#if needCandles}
        <p class="hint">{$t('journal.lab.needCandles')}</p>
      {/if}
    {/if}
    {#if outcome.warmup}
      <p class="warn">
        {outcome.warmup.bars == null
          ? $t('journal.lab.warmupCumulative', { n: outcome.warmup.trades.length })
          : $t('journal.lab.warmup', { n: outcome.warmup.trades.length, bars: outcome.warmup.bars })}
      </p>
    {/if}

    {#if !outcome.before.n}
      <p class="muted">{$t('journal.lab.nothing')}</p>
    {:else}
      {#if outcome.statement}
        <div class="insight {outcome.statement.severity}">
          <Icon name={SEVERITY_ICON[outcome.statement.severity] ?? 'lightbulb'} size={14} />
          <span>{statementText}</span>
        </div>
      {:else if outcome.before.n < minSample}
        <p class="warn">{$t('journal.lab.fewTrades', { n: outcome.before.n, min: minSample })}</p>
      {/if}

      <p class="detail">
        {#if d.kind === 'size'}
          {$t('journal.lab.size.detail', { factor: fmtNum(d.factor, 2), days: d.days_capped, skipped: d.trades_skipped })}
        {:else if d.kind === 'filter'}
          {$t('journal.lab.filter.detail', { kept: d.kept, removed: d.removed })}
        {:else if d.kind === 'reverse'}
          {$t('journal.lab.reverse.detail', { flipped: d.flipped })}
        {:else if d.kind === 'fees'}
          {$t('journal.lab.fees.detail', {
            name: d.schedule_name,
            paid: fmtMoney(d.paid, cur),
            alt: fmtMoney(d.alternative, cur),
            saving: fmtSignedMoney(d.saving, cur)
          })}
          {#if d.family}
            {$t('journal.lab.fees.familyDetail', { excluded: d.excluded, remainder: fmtNum(d.remainder_qty, 0) })}
          {/if}
        {/if}
      </p>

      <section>
        <h3>{$t('journal.lab.verdict')}</h3>
        <LabSummary before={outcome.before} after={outcome.after} currency={cur} {afterLabel} />
      </section>

      <section>
        <h3>{$t('journal.lab.curves')}</h3>
        <ExitCurves
          baseline={outcome.before.curve}
          baselineLabel={$t('journal.lab.col.before')}
          series={[{ label: afterLabel, color: colors.accent, curve: outcome.after.curve }]}
          currency={cur}
        />
      </section>

      <section>
        <h3>{$t('journal.lab.trades')}</h3>
        <LabTradeTable {rows} currency={cur} fees={kind === 'fees'} {afterLabel} />
      </section>
    {/if}
  {/if}
</div>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .form {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: var(--space-3);
  }
  .filter {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    flex-basis: 100%;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: var(--space-3);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .field input {
    width: 140px;
  }
  .check {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
  }
  .days {
    display: flex;
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .day {
    background: none;
    border: none;
    border-left: 0.5px solid var(--border);
    padding: var(--space-1) var(--space-2);
    font: inherit;
    font-size: var(--text-xs);
    color: var(--muted);
    cursor: pointer;
  }
  .day:first-child {
    border-left: none;
  }
  .day.on {
    background: var(--surface-2);
    color: var(--text);
  }
  .actions {
    margin-left: auto;
  }
  .hint,
  .muted,
  .detail {
    color: var(--muted);
    font-size: var(--text-sm);
    margin: 0;
  }
  .warn {
    color: var(--amber);
    font-size: var(--text-sm);
    margin: 0;
  }
  .link {
    background: none;
    border: none;
    padding: 0;
    color: var(--accent);
    font: inherit;
    cursor: pointer;
    text-decoration: underline;
  }
  .skipped {
    font-size: var(--text-sm);
    color: var(--muted);
    margin: 0;
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .reason {
    color: var(--text);
  }
  .insight {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    border: 0.5px solid var(--border);
    font-size: var(--text-sm);
    line-height: var(--lh-base);
  }
  .insight :global(svg) {
    flex: none;
    margin-top: 2px;
  }
  .insight.good :global(svg) {
    color: var(--green);
  }
  .insight.info :global(svg) {
    color: var(--accent);
  }
  .insight.warn :global(svg) {
    color: var(--amber);
  }
  section h3 {
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    margin: 0 0 var(--space-2);
  }
</style>
