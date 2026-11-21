<script>
  // Goals and FIRE: a target net worth and date, what a month costs, and what the curve says
  // about them. Growth is measured from the curve over at least 12 months; with less, the
  // projection says so instead of drawing a date from two points.
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import StatCard from '$lib/ui/StatCard.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Delta from '$lib/ui/charts/Delta.svelte';
  import { wealthApi } from './api.js';
  import { money, pct } from './view.js';
  import { t } from '$lib/i18n';

  let { settings = $bindable(), reloadKey = 0 } = $props();

  let goals = $state(null);
  let error = $state('');
  let saveError = $state('');
  let saving = $state(false);
  let form = $state({ goal_value: '', goal_date: '', monthly_expenses: '' });

  $effect(() => {
    form = {
      goal_value: settings?.goal_value ?? '',
      goal_date: settings?.goal_date ?? '',
      monthly_expenses: settings?.monthly_expenses ?? ''
    };
  });

  $effect(() => {
    void reloadKey;
    void settings;
    load();
  });

  async function load() {
    error = '';
    try {
      goals = await wealthApi.goals();
    } catch (e) {
      error = e.message;
    }
  }

  const num = (v) => (v === '' || v == null ? null : Number(v));

  async function save() {
    saving = true;
    saveError = '';
    try {
      settings = await wealthApi.savePreferences({
        goal_value: num(form.goal_value),
        goal_date: form.goal_date || null,
        monthly_expenses: num(form.monthly_expenses),
        stale_notify: settings.stale_notify,
        stale_channels: settings.stale_channels ?? [],
        cpi_series_id: settings.cpi_series_id ?? null
      });
    } catch (e) {
      saveError = e.message;
    }
    saving = false;
  }

  const cur = $derived(goals?.display_currency ?? settings?.display_currency ?? 'USD');
  const p = $derived(goals?.projection ?? null);
  const na = $derived($t('wealth.page.na'));
  // Progress toward the target net worth, 0..100 for the meter (the label keeps the true
  // share, which can pass 100).
  const progress = $derived(
    goals?.goal_value > 0 && goals?.net_worth != null ? (goals.net_worth / goals.goal_value) * 100 : null
  );
  const noHistory = $derived(goals?.growth_unavailable === 'insufficient_history');
  // One sentence, in words, of what the measured curve says about the goal.
  const sentence = $derived.by(() => {
    if (!goals) return '';
    const state = p?.state ?? 'no_goal';
    if (state === 'projected' && p?.date)
      return $t('wealth.goals.sentence.projected', {
        growth: pct(goals.growth_pct_year),
        target: money(goals.goal_value, cur),
        date: p.date
      });
    if (state === 'reached') return $t('wealth.goals.sentence.reached', { target: money(goals.goal_value, cur) });
    if (state === 'unreachable') return $t('wealth.goals.sentence.unreachable', { growth: pct(goals.growth_pct_year) });
    if (state === 'no_goal') return $t('wealth.goals.sentence.noGoal');
    return '';
  });
  const round1 = (v) => (v == null ? null : Math.round(v * 10) / 10);
</script>

<div class="goals">
  <form class="box form" onsubmit={(e) => { e.preventDefault(); save(); }}>
    <h3>{$t('wealth.goals.setTitle')}</h3>
    <div class="grid">
      <label class="field"><span>{$t('wealth.goals.goalValue', { currency: cur })}</span><input type="number" min="0" step="any" bind:value={form.goal_value} /></label>
      <label class="field"><span>{$t('wealth.goals.goalDate')}</span><input type="date" bind:value={form.goal_date} /></label>
      <label class="field"><span>{$t('wealth.goals.expenses', { currency: cur })}</span><input type="number" min="0" step="any" bind:value={form.monthly_expenses} /></label>
    </div>
    <ErrorText error={saveError} />
    <div class="actions"><button class="primary" type="submit" disabled={saving}>{saving ? $t('common.saving') : $t('common.save')}</button></div>
  </form>

  <ErrorText {error} />
  {#if goals}
    <section class="box progress">
      <header class="p-head">
        <h3>{$t('wealth.goals.progress')}</h3>
        {#if goals.goal_date}<span class="sub">{$t('wealth.goals.byDate', { date: goals.goal_date })}</span>{/if}
      </header>
      {#if progress != null}
        <div class="meter-line">
          <span class="fig">{money(goals.net_worth, cur)}</span>
          <span class="of">{$t('wealth.goals.ofTarget', { target: money(goals.goal_value, cur), pct: pct(progress) })}</span>
        </div>
        <!-- The fill carries the progress; the track is a lighter step of the same hue. -->
        <div class="meter" role="meter" aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.min(100, Math.round(progress))} aria-label={$t('wealth.goals.progress')}>
          <span class="fill" style:width="{Math.max(0, Math.min(100, progress))}%"></span>
        </div>
      {/if}
      {#if noHistory}
        <EmptyState icon="timeline" title={$t('wealth.goals.noProjection')} description={$t('wealth.goals.reason.insufficient_history')} compact />
      {:else}
        <p class="sentence">
          {sentence}
          {#if goals.on_track != null}
            <span class="track" class:ok={goals.on_track} class:late={!goals.on_track}>
              <span class="dot" aria-hidden="true"></span>{goals.on_track ? $t('wealth.goals.onTrack') : $t('wealth.goals.late', { date: goals.goal_date })}
            </span>
          {/if}
        </p>
      {/if}
    </section>

    <div class="tiles">
      <StatCard label={$t('wealth.page.netWorth')} value={money(goals.net_worth, cur)} />
      <StatCard
        label={$t('wealth.goals.fire')}
        value={money(goals.fire_number, cur, na)}
        hint={goals.fire_number != null ? $t('wealth.goals.fireProgress', { pct: pct(goals.fire_progress_pct) }) : $t('wealth.goals.needExpenses')}
      />
      <StatCard
        label={$t('wealth.goals.savings')}
        value={money(goals.savings_12m, cur, na)}
        hint={goals.savings_12m != null
          ? $t('wealth.goals.marketEffect', { amount: money(goals.market_effect_12m, cur) })
          : $t('wealth.goals.reason.insufficient_history')}
      />
      <div class="statcard-like">
        <span class="lbl">{$t('wealth.goals.savingsRate')}</span>
        <Delta value={round1(goals.savings_rate_pct)} digits={1} size="md" empty={na} />
        <span class="hint">
          {#if goals.savings_rate_pct == null}{goals.monthly_expenses ? $t('wealth.goals.reason.insufficient_history') : $t('wealth.goals.needExpenses')}
          {:else}{$t('wealth.goals.ofIncome')}{/if}
        </span>
      </div>
      <div class="statcard-like">
        <span class="lbl">{$t('wealth.goals.growth')}</span>
        <Delta value={round1(goals.growth_pct_year)} digits={1} size="md" empty={na} />
        <span class="hint">
          {#if goals.growth_unavailable}{$t(`wealth.goals.reason.${goals.growth_unavailable}`)}
          {:else}{$t('wealth.goals.over', { n: goals.history_months })}{/if}
        </span>
      </div>
    </div>
    <p class="method">{$t('wealth.goals.method')}</p>
  {/if}
</div>

<style>
  .goals {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .box {
    background: var(--bg);
    border: var(--hairline) solid var(--border);
    padding: var(--space-4);
  }
  h3 {
    font-size: 13px;
    font-weight: 600;
    margin: 0 0 var(--space-3);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: var(--space-3);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    margin-top: var(--space-3);
  }
  .progress {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .p-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .p-head h3 {
    margin: 0;
  }
  .sub {
    color: var(--dim);
    font-size: 12px;
  }
  .meter-line {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .fig {
    font-family: var(--mono);
    font-size: 15px;
    font-variant-numeric: tabular-nums;
  }
  .of {
    color: var(--dim);
    font-size: 12px;
  }
  .meter {
    height: 6px;
    border-radius: 3px;
    background: var(--accent-soft);
    overflow: hidden;
  }
  .fill {
    display: block;
    height: 100%;
    border-radius: 3px;
    background: var(--accent);
  }
  .sentence {
    margin: var(--space-1) 0 0;
    color: var(--text);
    font-size: 13px;
    line-height: var(--lh-base);
  }
  .track {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    margin-left: var(--space-2);
    font-size: 12px;
  }
  .track .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: currentColor;
  }
  .track.ok {
    color: var(--green-ink);
  }
  .track.late {
    color: var(--amber-ink);
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(5, minmax(0, 1fr));
    gap: var(--space-3);
  }
  /* A tile whose figure is a signed rate: StatCard's frame and type, a Delta inside. */
  .statcard-like {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    background: var(--bg);
    border: var(--hairline) solid var(--border);
    padding: var(--pad-metric);
    min-width: 0;
  }
  .lbl {
    color: var(--dim);
    font-size: 10px;
    font-weight: var(--fw-medium);
    line-height: var(--lh-tight);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .hint {
    color: var(--dim);
    font-size: var(--text-xs);
    line-height: var(--lh-tight);
  }
  .method {
    margin: 0;
    color: var(--dim);
    font-size: 12px;
    line-height: var(--lh-base);
  }
  @media (max-width: 1100px) {
    .tiles {
      grid-template-columns: repeat(3, minmax(0, 1fr));
    }
  }
  @media (max-width: 767px) {
    .grid {
      grid-template-columns: minmax(0, 1fr);
    }
    .tiles {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
    .actions button {
      min-height: 44px;
      flex: 1;
    }
  }
</style>
