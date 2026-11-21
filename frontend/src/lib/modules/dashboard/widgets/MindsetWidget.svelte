<script>
  // Mindset widget: today's check-in status, one row per active template. Shows whether
  // each is filled; the header redirect opens the full check-in. (Filling happens in the
  // module.) A day can hold several check-ins now, so this lists templates rather than the
  // two fixed phases it used to assume.
  import { mindsetApi, PHASES, isAnswered } from '$lib/modules/mindset/api.js';
  import { fmtLocal } from '$lib/ui/consistency.js';
  import { dateKey } from '$lib/format.js';
  import { t } from '$lib/i18n';
  import WidgetState from './WidgetState.svelte';
  import Stat from './parts/Stat.svelte';
  import MiniBars from './parts/MiniBars.svelte';
  import { dayLabels } from './parts/axis.js';
  import Heatmap from './parts/Heatmap.svelte';
  import Donut from './parts/Donut.svelte';
  import Icon from '$lib/ui/Icon.svelte';

  let { item, editing } = $props();
  const variant = $derived(item.config?.variant ?? 'today');

  // Read the day at fetch time, not at module init: a dashboard left open across
  // midnight would otherwise keep asking for yesterday. dateKey() builds the key from
  // local parts — `toLocaleDateString('en-CA')` worked only because that locale happens
  // to order its parts as ISO does.
  let day = $state(null);
  let marks = $state(null);
  let hist = $state.raw(null);
  let err = $state('');

  async function load() {
    err = '';
    try {
      day = await mindsetApi.day(dateKey());
    } catch (e) {
      err = e.message;
    }
  }
  $effect(() => {
    if (!editing && (variant === 'today' || variant === 'streak')) load();
  });

  // A year of day marks feeds both the streak figure and the history grid.
  $effect(() => {
    if (editing || !['streak', 'history'].includes(variant)) return;
    const to = new Date();
    const from = new Date(to);
    from.setDate(from.getDate() - 364);
    let alive = true;
    mindsetApi
      .listMarks(fmtLocal(from), fmtLocal(to))
      .then((m) => { if (alive) marks = m; })
      .catch((e) => { if (alive) err = e.message; });
    return () => { alive = false; };
  });

  // The answer breakdown reads the recent entries and counts one prompt's answers.
  $effect(() => {
    if (editing || variant !== 'breakdown') return;
    let alive = true;
    mindsetApi
      .history(item.config?.days ?? 90)
      .then((h) => { if (alive) hist = h; })
      .catch((e) => { if (alive) err = e.message; });
    return () => { alive = false; };
  });

  const markDays = $derived(new Set((marks ?? []).map((m) => m.day)));
  const streak = $derived.by(() => {
    const d = new Date();
    d.setHours(0, 0, 0, 0);
    if (!markDays.has(fmtLocal(d))) d.setDate(d.getDate() - 1);
    let n = 0;
    while (markDays.has(fmtLocal(d))) {
      n++;
      d.setDate(d.getDate() - 1);
    }
    return n;
  });
  const streakBars = $derived.by(() => {
    const out = [];
    const d = new Date();
    d.setHours(0, 0, 0, 0);
    for (let i = 27; i >= 0; i--) {
      const day2 = new Date(d);
      day2.setDate(d.getDate() - i);
      out.push(markDays.has(fmtLocal(day2)) ? 1 : 0.15);
    }
    return out;
  });
  // The grid shows presence, not magnitude: a marked day is a day, so every cell that
  // exists carries the same weight.
  const heatDays = $derived(new Map((marks ?? []).map((m) => [m.day, m.mark === 'full' ? 1 : 0.5])));

  // The prompt the breakdown counts: the configured one, else the first closed-answer
  // prompt (a free-text answer has no categories to count).
  const prompt = $derived(
    (hist?.prompts ?? []).find((p) => p.id === item.config?.promptId) ??
      (hist?.prompts ?? []).find((p) => p.kind === 'choice' || p.kind === 'scale') ??
      null
  );
  const answerCounts = $derived.by(() => {
    if (!prompt) return [];
    const counts = new Map();
    for (const e of hist?.entries ?? []) {
      const v = e.answers?.[prompt.id];
      if (!isAnswered(v)) continue;
      for (const one of Array.isArray(v) ? v : [v]) {
        const key = String(one);
        counts.set(key, (counts.get(key) ?? 0) + 1);
      }
    }
    return [...counts].map(([label, value]) => ({ label, value })).sort((a, b) => b.value - a.value);
  });

  const templates = $derived(day?.templates ?? []);

  /// A check-in counts as done once every one of its prompts carries an answer.
  function answered(tpl) {
    const answers = (day?.entries ?? []).find((x) => x.template_id === tpl.id)?.answers ?? {};
    const prompts = (day?.prompts ?? []).filter((p) => p.template_id === tpl.id);
    return prompts.length > 0 && prompts.every((p) => isAnswered(answers[p.id]));
  }

  const iconOf = (phase) => PHASES.find((p) => p.key === phase)?.icon ?? '';
</script>

<WidgetState
  {editing}
  error={err}
  loading={(variant === 'today' && day === null) || (['streak', 'history'].includes(variant) && marks === null) || (variant === 'breakdown' && hist === null)}
  empty={variant === 'today' && templates.length === 0}
  preview={$t('dashboard.widgets.mindset.preview')}
  emptyText={$t('mindset.page.noTemplates')}
  rows={2}
>
  {#if variant === 'streak'}
    <div class="w-body">
      <Stat value={String(streak)} note={$t('dashboard.widgets.mindset.daysInARow')} />
      <MiniBars values={streakBars} labels={dayLabels(streakBars.length)} tone="pos" height={52}
        valueFormat={(v) => (v === 1 ? $t('common.done') : $t('dashboard.widgets.mindset.notFilled'))}
        label={$t('dashboard.widgets.mindset.daysInARow')} />
    </div>
  {:else if variant === 'history'}
    <Heatmap days={heatDays} signed={false} weeks={item.config?.weeks ?? 53}
      valueFormat={(v) => (v === 1 ? $t('common.done') : $t('dashboard.widgets.mindset.partly'))}
      emptyText={$t('dashboard.widgets.mindset.notFilled')}
      label={$t('dashboard.widgets.mindset.history')} />
  {:else if variant === 'breakdown'}
    {#if !prompt || answerCounts.length === 0}
      <p class="w-state">{$t('dashboard.widgets.mindset.noAnswers')}</p>
    {:else}
      <div class="w-body">
        <span class="w-eyebrow">{prompt.label}</span>
        <Donut segments={answerCounts} centerLabel={$t('dashboard.widgets.mindset.entries')} />
      </div>
    {/if}
  {:else}
  <ul class="w-list">
    {#each templates as tpl (tpl.id)}
      {@const done = answered(tpl)}
      <li class="w-row">
        <span class="mark" class:on={done}><Icon name="check-circle" size={16} /></span>
        <span class="ic" aria-hidden="true">{iconOf(tpl.phase)}</span>
        <span class="w-name">{tpl.name}</span>
        <span class="w-pill" class:pos={done}>
          {done ? $t('dashboard.widgets.mindset.done') : $t('dashboard.widgets.mindset.notFilled')}
        </span>
      </li>
    {/each}
  </ul>
  {/if}
</WidgetState>

<style>
  .mark {
    display: inline-flex;
    flex-shrink: 0;
    color: var(--faint);
  }
  .mark.on {
    color: var(--green);
  }
  .ic {
    flex-shrink: 0;
    font-size: 14px;
    line-height: 1;
  }
  .w-name {
    flex: 1;
  }
</style>
