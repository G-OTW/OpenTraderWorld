<script>
  // The shared insights page: predefined researchers over one source (the journal, a
  // backtest run, a paper session). It reads like a research note: the strongest finding
  // first, one sentence per card with its evidence, and the one action that measures it
  // (the journal lab, or the strategy with the filter applied as an unsaved draft).
  // The server decides what fires; this page only words and formats it.
  import { page } from '$app/stores';
  import Icon from '$lib/ui/Icon.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { t, locale } from '$lib/i18n';
  import { fmtMoney, fmtSignedMoney, fmtNum, fmtPct, fmtSignedPct } from '$lib/format.js';
  import { installedIds, ensureInstalled } from '$lib/modules/installed.js';
  import { journalApi } from '$lib/modules/journal/api.js';
  import { SEVERITY_ICON } from '$lib/modules/journal/insights.js';
  import { insightsApi } from './api.js';
  import {
    sourceFromQuery,
    bodyOf,
    SOURCE_MODULE,
    insightParams,
    journalParams,
    evidenceParts,
    actionOf,
    weekdayName,
    duration
  } from './insights.js';

  const parsed = $derived(sourceFromQuery($page.url.searchParams));
  const source = $derived(parsed.source ?? null);
  const needed = $derived(source ? SOURCE_MODULE[source.kind] : null);
  const missing = $derived(!!needed && $installedIds != null && !$installedIds.has(needed));

  let data = $state(null);
  let error = $state('');
  let loading = $state(false);
  let categoryName = $state('');

  $effect(() => {
    ensureInstalled().catch(() => {});
  });

  // One request per address. A late answer for an address left behind is dropped.
  let seq = 0;
  $effect(() => {
    const src = source;
    const blocked = missing || $installedIds == null;
    if (!src || blocked) return;
    const mine = ++seq;
    loading = true;
    error = '';
    insightsApi
      .run(bodyOf(src, new Date().getTimezoneOffset()))
      .then((d) => mine === seq && (data = d))
      .catch((e) => mine === seq && ((error = e.message), (data = null)))
      .finally(() => mine === seq && (loading = false));
  });

  $effect(() => {
    const id = source?.kind === 'journal' ? source.filters?.category_id : null;
    categoryName = '';
    if (!id) return;
    journalApi
      .listCategories()
      .then((cats) => (categoryName = cats.find((c) => c.id === id)?.name ?? ''))
      .catch(() => {});
  });

  const cur = $derived(data?.currency ?? null);
  const plain = (v) => fmtNum(v, 2);
  const signedPlain = (v) => `${v > 0 ? '+' : v < 0 ? '−' : ''}${fmtNum(Math.abs(v), 2)}`;
  const units = $derived({
    min: (n) => $t('insights.unit.min', { n }),
    hour: (n) => $t('insights.unit.hour', { n }),
    day: (n) => $t('insights.unit.day', { n })
  });
  const fmt = $derived({
    money: (v) => (cur ? fmtMoney(v, cur) : plain(v)),
    signedMoney: (v) => (cur ? fmtSignedMoney(v, cur) : signedPlain(v)),
    int: (v) => fmtNum(Math.round(v), 0),
    num: (v) => fmtNum(v, 2),
    pct: (v) => fmtPct(v, 1),
    signedPct: (v) => fmtSignedPct(v, 1),
    factor: (v) => `×${fmtNum(v, 2)}`,
    ratio: (v) => `${fmtNum(v, 1)}×`,
    hour: (v) => `${String(v).padStart(2, '0')}:00`,
    weekday: (v) => weekdayName(v, $locale),
    minutes: (v) => duration(v, units, (x) => fmtNum(x, 1))
  });
  const sideLabel = (label, key) => (key === 'side_drag' ? $t(`insights.side.${label}`) : label);
  const statement = (i) => $t(`insights.statement.${i.key}`, insightParams(i, fmt, sideLabel));
  const evidence = (i) => evidenceParts(i.evidence, fmt).map(([k, p]) => $t(k, p));
  const journalText = (i, prefix) => $t(`${prefix}.${i.key}`, journalParams(i, fmt));

  const title = $derived.by(() => {
    if (!source) return '';
    if (source.kind === 'journal') return categoryName ? $t('insights.source.journalIn', { name: categoryName }) : $t('insights.source.journal');
    const name = data?.source?.name ?? '';
    return $t(`insights.source.${source.kind}`, { name });
  });
  const back = $derived.by(() => {
    if (!source) return '/';
    if (source.kind === 'journal') return '/journal';
    if (source.kind === 'backtest') return `/backtest?run=${source.run}`;
    return `/backtest/paper/${source.session}`;
  });

  const findings = $derived(data?.insights ?? []);
  const journalOwn = $derived([
    ...(data?.behavior ?? []).map((i) => ({ ...i, prefix: 'journal.behavior.insight' })),
    ...(data?.exposure ?? []).map((i) => ({ ...i, prefix: 'journal.exposure.warning' }))
  ]);
  const few = $derived(!!data && data.sample < data.min_sample);
</script>

<div class="insights-page">
  <header class="head">
    <a class="back" href={back}><Icon name="arrow-left" size={14} /> {$t('insights.back')}</a>
    <div class="titles">
      <h1>{$t('insights.title')}</h1>
      {#if title}<p class="sub">{title}</p>{/if}
    </div>
  </header>

  {#if parsed.error}
    <p class="note">{$t(parsed.error)}</p>
  {:else if missing}
    <div class="note card">
      <p>{$t('insights.missing', { module: $t(`insights.module.${needed}`) })}</p>
      <a href="/settings">{$t('insights.openSettings')}</a>
    </div>
  {:else}
    <ErrorText {error} />
    {#if loading && !data}
      <p class="note" aria-live="polite">{$t('insights.loading')}</p>
    {/if}
    {#if data}
      <p class="sample">
        {$t('insights.sample', { n: fmt.int(data.sample) })}
        {#if data.unmeasured > 0}{$t('insights.unmeasured', { n: fmt.int(data.unmeasured) })}{/if}
        {#if data.currency == null}{$t('insights.accountCurrency')}{/if}
      </p>

      {#if few}
        <div class="empty card">
          <Icon name="lightbulb" size={20} />
          <p>{$t('insights.empty.few', { min: fmt.int(data.min_sample), n: fmt.int(data.sample) })}</p>
        </div>
      {:else if !findings.length}
        <div class="empty card">
          <Icon name="check-circle" size={20} />
          <p>{$t('insights.empty.none', { n: fmt.int(data.sample) })}</p>
        </div>
      {:else}
        <ol class="cards">
          {#each findings as i, n (i.key)}
            {@const act = actionOf(i, source)}
            <li class="card finding {i.severity}" class:lead={n === 0}>
              <span class="icon"><Icon name={SEVERITY_ICON[i.severity] ?? 'lightbulb'} size={n === 0 ? 18 : 15} /></span>
              <div class="body">
                <p class="text">{statement(i)}</p>
                <p class="evidence">{evidence(i).join(' · ')}</p>
              </div>
              {#if act}
                <a class="act" href={act.href}>
                  <Icon name={act.kind === 'lab' ? 'flask' : 'filter'} size={13} />
                  {$t(act.kind === 'lab' ? 'insights.action.lab' : 'insights.action.filter')}
                </a>
              {:else if i.key === 'fees_no_schedule'}
                <a class="act" href="/journal?view=fees">
                  <Icon name="coins" size={13} />
                  {$t('insights.action.addSchedule')}
                </a>
              {/if}
            </li>
          {/each}
        </ol>
      {/if}

      {#if journalOwn.length}
        <section class="journal-own">
          <h2>{$t('insights.journalOwn')}</h2>
          <ul class="cards">
            {#each journalOwn as i, n (n)}
              <li class="card finding {i.severity}">
                <span class="icon"><Icon name={SEVERITY_ICON[i.severity] ?? 'lightbulb'} size={15} /></span>
                <div class="body"><p class="text">{journalText(i, i.prefix)}</p></div>
              </li>
            {/each}
          </ul>
        </section>
      {/if}
    {/if}
  {/if}
</div>

<style>
  .insights-page {
    max-width: 860px;
    margin: 0 auto;
    padding: var(--space-6) var(--space-4) var(--space-8);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .head {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .back {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    color: var(--muted);
    font-size: var(--text-sm);
    text-decoration: none;
    align-self: flex-start;
  }
  .back:hover {
    color: var(--text);
  }
  h1 {
    font-size: var(--text-lg);
    font-weight: var(--fw-medium);
    margin: 0;
  }
  h2 {
    font-size: var(--text-base);
    font-weight: var(--fw-medium);
    margin: 0 0 var(--space-2);
    color: var(--muted);
  }
  .sub,
  .sample,
  .note {
    color: var(--muted);
    font-size: var(--text-sm);
    margin: 0;
  }
  .card {
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    padding: var(--space-3) var(--space-4);
  }
  .note.card {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .empty {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    color: var(--muted);
  }
  .empty p {
    margin: 0;
  }
  .cards {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .finding {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
  }
  .finding .icon {
    color: var(--muted);
    padding-top: 2px;
    flex: none;
  }
  .finding.warn .icon {
    color: var(--amber);
  }
  .finding.good .icon {
    color: var(--green);
  }
  .body {
    flex: 1;
    min-width: 0;
  }
  .text {
    margin: 0;
    line-height: 1.5;
    color: var(--text);
  }
  .lead .text {
    font-size: var(--text-md);
  }
  .evidence {
    margin: var(--space-1) 0 0;
    color: var(--muted);
    font-size: var(--text-xs);
    font-variant-numeric: tabular-nums;
  }
  .act {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-2);
    color: var(--text);
    font-size: var(--text-xs);
    text-decoration: none;
    white-space: nowrap;
  }
  .act:hover {
    background: var(--surface-2);
  }
  .journal-own {
    margin-top: var(--space-4);
  }
  @media (max-width: 640px) {
    .insights-page {
      padding: var(--space-4) var(--space-4) var(--space-8);
    }
    .finding {
      flex-wrap: wrap;
    }
    .act {
      margin-left: calc(15px + var(--space-3));
    }
  }
</style>
