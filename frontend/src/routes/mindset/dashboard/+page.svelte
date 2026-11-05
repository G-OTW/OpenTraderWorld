<script>
  // Dashboard: the mindset read over the chosen window. The raw check-ins are on the
  // History page.
  //
  // Every scored answer lands on 0..100 (see scoring.js), the day's answers average into
  // one score, and the dashboard reads that series: where it stands against the previous
  // window, the days outside the usual band, the answers far from their question's norm,
  // which questions improve or regress between the first and second half of the window,
  // and how the score lines up with realized P&L in a journal of the user's choice.
  import { goto } from '$app/navigation';
  import ModuleNav from '$lib/ui/ModuleNav.svelte';
  import PageHeader from '$lib/ui/PageHeader.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import StatCard from '$lib/ui/StatCard.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import ScoreChart from '$lib/modules/mindset/ScoreChart.svelte';
  import Heatmap from '$lib/modules/mindset/Heatmap.svelte';
  import Correlation from '$lib/modules/mindset/Correlation.svelte';
  import WordCloud from '$lib/modules/mindset/WordCloud.svelte';
  import { mindsetApi, NAV_LINKS } from '$lib/modules/mindset/nav.js';
  import { buildDashboard, addDays, roundOf, MIN_DAYS } from '$lib/modules/mindset/scoring.js';
  import { wordClouds } from '$lib/modules/mindset/text.js';
  import { t } from '$lib/i18n';

  // Ranges named by span, not by a raw day count.
  const RANGES = [
    { days: 30, key: 'mindset.history.range1m' },
    { days: 60, key: 'mindset.history.range2m' },
    { days: 180, key: 'mindset.history.range6m' },
    { days: 365, key: 'mindset.history.range1y' }
  ];

  let hist = $state(null);
  let error = $state('');
  let span = $state(60);

  const today = new Date().toISOString().slice(0, 10);
  const to = today;
  const from = $derived(addDays(today, -(span - 1)));
  // The previous window rides along so the headline delta needs no second request.
  const fetchFrom = $derived(addDays(from, -span));

  // The effect below runs once on mount, so no separate onMount load.
  function load() {
    error = '';
    mindsetApi
      .history(fetchFrom, to)
      .then((r) => (hist = r))
      .catch((e) => (error = e.message));
  }
  $effect(() => {
    fetchFrom;
    load();
  });

  const promptById = $derived(new Map((hist?.prompts ?? []).map((p) => [p.id, p])));
  const templateById = $derived(new Map((hist?.templates ?? []).map((tpl) => [tpl.id, tpl])));
  const templateName = (p) => templateById.get(p?.template_id)?.name ?? '';

  const dash = $derived(hist ? buildDashboard(hist, { from, to }) : null);
  // Word clouds by score, shown here only while no journal is being compared.
  let compared = $state(false);
  const clouds = $derived(dash ? wordClouds(dash.days, (d) => (dash.mean == null ? null : d.score >= dash.mean)) : null);
  const scoreByDate = $derived(new Map((dash?.days ?? []).map((d) => [d.date, d.score])));

  const open = (date) => goto(`/mindset?date=${date}`);

  function fmtDate(iso, long = true) {
    return new Date(`${iso}T00:00:00`).toLocaleDateString(undefined, long
      ? { weekday: 'short', day: 'numeric', month: 'long', year: 'numeric' }
      : { day: 'numeric', month: 'short' });
  }
  const signed = (v) => (v == null ? '' : `${v > 0 ? '+' : v < 0 ? '−' : ''}${Math.abs(Math.round(v))}`);
  const trendIcon = { up: 'arrow-up', down: 'arrow-down', flat: 'minus' };
</script>

<div class="page">
  <ModuleNav links={NAV_LINKS} label="mindset.nav.label" />

  <PageHeader
    title={$t('mindset.dashboard.title')}
    subtitle={dash ? $t('mindset.history.subtitle', { days: dash.days.length, from: fmtDate(from, false), to: fmtDate(to, false) }) : ''}
  >
    {#snippet actions()}
      <div class="limits" role="group" aria-label={$t('mindset.history.range')}>
        <span class="rlbl">{$t('mindset.history.range')}</span>
        {#each RANGES as r (r.days)}
          <button class="chip" class:active={span === r.days} onclick={() => (span = r.days)}>
            {$t(r.key)}
          </button>
        {/each}
      </div>
    {/snippet}
  </PageHeader>

  <ErrorText error={error} copyable />

  {#if !hist}
    <Skeleton height="200px" />
  {:else if dash.days.length === 0 && hist.entries.length === 0}
    <div class="fill">
      <EmptyState icon="bar-chart" title={$t('mindset.history.emptyTitle')} description={$t('mindset.page.noCheckinsYet')}>
        {#snippet action()}
          <a class="btn primary link" href="/mindset">{$t('mindset.history.goToCheckin')}</a>
        {/snippet}
      </EmptyState>
    </div>
  {:else}
      {#if dash.scoredPrompts === 0}
        <section class="panel">
          <p class="muted">{$t('mindset.history.notScored')}</p>
          <a class="btn link" href="/mindset/templates">{$t('mindset.nav.templates')}</a>
        </section>
      {:else}
        <!-- Headline: where the window stands. -->
        <div class="kpis">
          <StatCard
            label={$t('mindset.history.kpiScore')}
            value={dash.mean == null ? '·' : roundOf(dash.mean)}
            delta={dash.delta == null ? null : Math.round(dash.delta)}
            deltaSuffix={dash.delta == null ? '' : ` ${$t('mindset.history.kpiVsPrev')}`}
            hint={$t('mindset.history.kpiScoreHint')}
          />
          <StatCard
            label={$t('mindset.history.kpiBest')}
            value={dash.best ? roundOf(dash.best.score) : '·'}
            hint={dash.best ? fmtDate(dash.best.date, false) : ''}
          />
          <StatCard
            label={$t('mindset.history.kpiWorst')}
            value={dash.worst ? roundOf(dash.worst.score) : '·'}
            hint={dash.worst ? fmtDate(dash.worst.date, false) : ''}
          />
          <StatCard
            label={$t('mindset.history.kpiStreak')}
            value={dash.streak.current}
            hint={$t('mindset.history.kpiStreakHint', { best: dash.streak.best, good: dash.streak.good })}
          />
        </div>

        <section class="panel">
          <h2>{$t('mindset.history.scoreChart')}</h2>
          {#if !dash.enough}
            <p class="muted">{$t('mindset.history.tooFew', { min: MIN_DAYS })}</p>
          {/if}
          <ScoreChart days={dash.days} mean={dash.mean} std={dash.enough ? dash.std : 0} {from} {to} onpick={open} />
          {#if dash.enough}
            <p class="legend">
              <span class="sw band"></span>{$t('mindset.history.scoreBand')}
              <span class="sw high"></span>{$t('mindset.history.extremeHigh')}
              <span class="sw low"></span>{$t('mindset.history.extremeLow')}
            </p>
          {/if}
        </section>

        {#if dash.enough}
          <div class="two">
            <!-- Days outside the band. -->
            <section class="panel">
              <h2>{$t('mindset.history.extremes')}</h2>
              <p class="hint">{$t('mindset.history.extremesHint')}</p>
              {#if dash.extremes.length === 0}
                <p class="muted">{$t('mindset.history.none')}</p>
              {:else}
                <ul class="list">
                  {#each dash.extremes as d (d.date)}
                    <li>
                      <button class="rowbtn" onclick={() => open(d.date)}>
                        <span class="dot {d.dir}"></span>
                        <span class="rl">{fmtDate(d.date)}</span>
                        <span class="rv num">{roundOf(d.score)}</span>
                        <span class="rd num {d.dir}">{signed(d.delta)}</span>
                      </button>
                    </li>
                  {/each}
                </ul>
              {/if}
            </section>

            <!-- Answers far from their question's norm. -->
            <section class="panel">
              <h2>{$t('mindset.history.irregular')}</h2>
              <p class="hint">{$t('mindset.history.irregularHint')}</p>
              {#if dash.irregular.length === 0}
                <p class="muted">{$t('mindset.history.none')}</p>
              {:else}
                <ul class="list">
                  {#each dash.irregular as a (a.promptId + a.date)}
                    <li>
                      <button class="rowbtn" onclick={() => open(a.date)}>
                        <span class="dot {a.delta > 0 ? 'high' : 'low'}"></span>
                        <span class="rl">
                          <span class="rq">{promptById.get(a.promptId)?.label}</span>
                          <span class="rs">{fmtDate(a.date, false)} · {$t('mindset.history.irregularUsual', { mean: roundOf(a.mean) })}</span>
                        </span>
                        <span class="rv num">{roundOf(a.value)}</span>
                        <span class="rd num {a.delta > 0 ? 'high' : 'low'}">{signed(a.delta)}</span>
                      </button>
                    </li>
                  {/each}
                </ul>
              {/if}
            </section>
          </div>

          <section class="panel">
            <h2>{$t('mindset.history.heatmap')}</h2>
            <Heatmap questions={dash.questions} days={dash.days} {templateName} onpick={open} />
          </section>

          {#if !compared}
            <section class="panel">
              <h2>{$t('mindset.history.cloudTitle')}</h2>
              <p class="hint">{$t('mindset.history.cloudHint')}</p>
              <WordCloud
                good={clouds.good}
                bad={clouds.bad}
                docs={clouds.docs}
                goodLabel={$t('mindset.history.wordsGood')}
                badLabel={$t('mindset.history.wordsBad')}
              />
            </section>
          {/if}

          <section class="panel">
            <h2>{$t('mindset.history.progress')}</h2>
            <p class="hint">{$t('mindset.history.progressHint')}</p>
            <table class="prog">
              <thead>
                <tr>
                  <th>{$t('mindset.history.colQuestion')}</th>
                  <th class="num">{$t('mindset.history.colMean')}</th>
                  <th class="num">{$t('mindset.history.colFirst')}</th>
                  <th class="num">{$t('mindset.history.colSecond')}</th>
                  <th class="num">{$t('mindset.history.colDelta')}</th>
                  <th></th>
                </tr>
              </thead>
              <tbody>
                <tr class="overall">
                  <td>{$t('mindset.history.overall')}</td>
                  <td class="num">{roundOf(dash.mean) ?? '·'}</td>
                  <td class="num">{roundOf(dash.overall.first) ?? '·'}</td>
                  <td class="num">{roundOf(dash.overall.second) ?? '·'}</td>
                  <td class="num {dash.overall.delta > 0 ? 'high' : dash.overall.delta < 0 ? 'low' : ''}">{signed(dash.overall.delta)}</td>
                  <td></td>
                </tr>
                {#each dash.questions as q (q.prompt.id)}
                  <tr>
                    <td>
                      <span class="rq">{q.prompt.label}</span>
                      <span class="rs">{templateName(q.prompt)} · {$t('mindset.history.nAnswers', { n: q.n })}</span>
                    </td>
                    <td class="num">{roundOf(q.mean)}</td>
                    <td class="num">{roundOf(q.first) ?? '·'}</td>
                    <td class="num">{roundOf(q.second) ?? '·'}</td>
                    <td class="num {q.trend === 'up' ? 'high' : q.trend === 'down' ? 'low' : ''}">{signed(q.delta)}</td>
                    <td>
                      {#if q.trend}
                        <span class="tag {q.trend}">
                          <Icon name={trendIcon[q.trend]} size={12} />
                          {$t(`mindset.history.trend_${q.trend}`)}
                        </span>
                      {/if}
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </section>

          <section class="panel">
            <h2>{$t('mindset.history.correlation')}</h2>
            <p class="hint">{$t('mindset.history.correlationHint')}</p>
            <Correlation days={dash.days} mean={dash.mean} extremes={dash.extremes} {from} {to} onpick={open} bind:compared />
          </section>
        {/if}
      {/if}
  {/if}
</div>

<style>
  .page {
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: var(--space-6);
    gap: var(--space-4);
    overflow-y: auto;
  }

  .limits {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .rlbl {
    font-size: var(--text-xs);
    color: var(--muted);
  }

  .fill {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .btn {
    background: var(--surface-2);
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-3);
    color: var(--text);
    font-size: var(--text-base);
    cursor: pointer;
  }
  .btn.primary {
    border-color: var(--border-control);
    font-weight: var(--fw-medium);
  }
  .btn.link {
    text-decoration: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    align-self: flex-start;
  }

  .kpis {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: var(--space-3);
  }

  .panel {
    background: var(--surface);
    border: 0.5px solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-width: 0;
  }
  .panel h2 {
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--muted);
  }
  .panel h2 + .hint {
    margin-top: calc(-1 * var(--space-2));
  }
  .hint {
    font-size: var(--text-xs);
    color: var(--dim);
  }
  .two {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(320px, 1fr));
    gap: var(--space-4);
  }

  .legend {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    color: var(--dim);
  }
  .sw {
    width: 12px;
    height: 12px;
    border-radius: 2px;
    display: inline-block;
    margin-left: var(--space-2);
  }
  .sw.band {
    background: color-mix(in srgb, var(--chart-1) 20%, transparent);
    margin-left: 0;
  }
  .sw.high {
    background: var(--green);
    border-radius: 50%;
  }
  .sw.low {
    background: var(--red);
    border-radius: 50%;
  }

  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
  }
  .rowbtn {
    width: 100%;
    display: grid;
    grid-template-columns: 10px minmax(0, 1fr) auto auto;
    align-items: center;
    gap: var(--space-3);
    background: transparent;
    border: none;
    border-bottom: var(--hairline) solid var(--border);
    padding: var(--space-2) 0;
    color: var(--text);
    text-align: left;
    cursor: pointer;
    font-size: var(--text-sm);
  }
  .list li:last-child .rowbtn {
    border-bottom: none;
  }
  .rowbtn:hover .rl {
    color: var(--accent);
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--muted);
  }
  .dot.high {
    background: var(--green);
  }
  .dot.low {
    background: var(--red);
  }
  .rl {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .rq {
    display: block;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .rs {
    display: block;
    font-size: var(--text-xs);
    color: var(--dim);
  }
  .rv {
    font-weight: var(--fw-medium);
  }
  .rd {
    min-width: 3ch;
    text-align: right;
    color: var(--muted);
  }
  .high {
    color: var(--green);
  }
  .low {
    color: var(--red);
  }

  .prog {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }
  .prog th,
  .prog td {
    padding: var(--space-2) var(--space-2);
    border-bottom: var(--hairline) solid var(--border);
    text-align: left;
    vertical-align: middle;
  }
  .prog th {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--dim);
    font-weight: var(--fw-medium);
  }
  .prog .num {
    text-align: right;
    font-family: var(--mono);
    white-space: nowrap;
  }
  .prog .overall td {
    font-weight: var(--fw-medium);
  }
  .tag {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: var(--text-xs);
    white-space: nowrap;
    color: var(--muted);
  }
  .tag.up {
    color: var(--green);
  }
  .tag.down {
    color: var(--red);
  }

  .muted {
    color: var(--muted);
  }
  .small {
    font-size: var(--text-xs);
  }
  @media (max-width: 767px) {
    .page {
      padding: var(--space-4);
      padding-bottom: calc(80px + env(safe-area-inset-bottom));
    }
    .limits {
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      width: 100%;
    }
    .rlbl {
      grid-column: 1 / -1;
    }
    .limits .chip {
      min-width: 0;
      min-height: 44px;
      white-space: normal;
    }
    .two {
      grid-template-columns: minmax(0, 1fr);
    }
    .prog th:nth-child(3),
    .prog td:nth-child(3),
    .prog th:nth-child(4),
    .prog td:nth-child(4) {
      display: none;
    }
    .rowbtn {
      min-height: 44px;
    }
  }
</style>
