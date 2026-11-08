<script>
  // `stress`: pick a shock, see what it does, and read the coverage before the headline.
  //
  // The block itself is the readiness report (which factors have a proxy, which positions
  // have a measured sensitivity, which have none). Running a scenario is a POST, because the
  // caller chooses it. The number is only ever true of the share of the book it covers, so
  // coverage is stated first and not as a footnote.
  import { t } from '$lib/i18n';
  import { fmtPct, fmtSignedPct, fmtRatioPct, fmtFixed } from '$lib/format.js';
  import Icon from '$lib/ui/Icon.svelte';
  import Delta from '$lib/ui/charts/Delta.svelte';
  import { fmtMoney, fmtSignedMoney, na } from '../api.js';
  import Unavailable from './Unavailable.svelte';
  import Sample from './Sample.svelte';
  import ScenarioEditor from './ScenarioEditor.svelte';
  import { portfoliosApi } from '../api.js';

  let { block = null, currency = 'USD', portfolioId = null, onfix = null } = $props();

  const d = $derived(block?.data ?? null);
  let scenarios = $state([]);
  let running = $state(null);
  let result = $state(null);
  let error = $state(null);
  // The scenario open in the editor: a row, `'new'`, or null when closed.
  let editing = $state(null);

  function load() {
    portfoliosApi
      .scenarios()
      .then((s) => (scenarios = s))
      .catch((e) => (error = e.message));
  }

  $effect(() => {
    if (block?.status !== 'ok') return;
    load();
  });

  async function run(s) {
    running = s.id;
    error = null;
    try {
      result = await portfoliosApi.stress(portfolioId, { scenario_id: s.id });
    } catch (e) {
      error = e.message;
      result = null;
    } finally {
      running = null;
    }
  }

  const impact = $derived(result?.impact ?? null);

  // Below this share of the *invested* book measured, there is no headline to print. A
  // number standing on a third of the positions is not a cautious estimate, it is a
  // different portfolio's answer wearing this one's label.
  const MIN_COVERAGE = 0.5;

  // Weight of the positions the shock could not reach, as a share of the book.
  const unmeasuredPct = $derived(
    impact ? impact.unexplained.reduce((a, u) => a + (u.weight_pct ?? 0), 0) : 0
  );
  // Measured over what could ever be measured. `coverage` is a share of net worth, and cash
  // is never covered: on a book that is 60% cash, a fully measured 40% would read as 0.4 and
  // trip every floor below it. Cash not moving is an answer, not a gap.
  const invested = $derived(impact ? impact.coverage + unmeasuredPct / 100 : 0);
  const measured = $derived(invested > 1e-9 ? impact.coverage / invested : 1);

  const unquantifiable = $derived(impact != null && measured < MIN_COVERAGE);
  const partial = $derived(impact != null && measured < 0.9);
  const names = $derived(impact ? impact.unexplained.map((u) => u.symbol).join(', ') : '');
</script>

{#if block?.status !== 'ok' || !d}
  <Unavailable missing={block?.missing ?? []} sample={block?.sample} {onfix} />
{:else}
  <div class="pa-stack">
  <div class="scenarios">
    {#each scenarios as s (s.id)}
      <div class="scen" class:active={result?.scenario?.name === s.name}>
        <button class="run" onclick={() => run(s)} disabled={running}>
          <span class="name">{s.name}</span>
          <span class="kind">{$t(`portfolios.analytics.stress.kind.${s.kind}`)}</span>
          {#if running === s.id}<span class="spin"><Icon name="refresh-cw" size={12} /></span>{/if}
        </button>
        <button
          class="edit"
          onclick={() => (editing = s)}
          aria-label={$t('portfolios.analytics.stress.editScenario')}
          title={$t('portfolios.analytics.stress.editScenario')}
        >
          <Icon name="pencil" size={12} />
        </button>
      </div>
    {/each}
    <button class="scen add" onclick={() => (editing = 'new')}>
      <Icon name="plus" size={12} />
      {$t('portfolios.analytics.stress.newScenario')}
    </button>
  </div>

  {#if editing}
    <ScenarioEditor scenario={editing === 'new' ? null : editing} onclose={() => (editing = null)} onsaved={load} />
  {/if}

  {#if error}<p class="err">{error}</p>{/if}

  {#if unquantifiable}
    <!-- One sentence, and no number. The two causes have two different remedies, so the
         sentence names the one that applies rather than saying "uncertain". -->
    <div class="result unmeasured">
      <div class="headline">
        <span class="lbl">{result.scenario.name}</span>
      </div>
      <p class="warn">
        {$t('portfolios.analytics.stress.unquantifiable', {
          names,
          pct: fmtPct(unmeasuredPct, 0)
        })}
      </p>
      <p class="hint">
        {$t(
          result.scenario.kind === 'historical'
            ? 'portfolios.analytics.stress.unquantifiableFixHistorical'
            : 'portfolios.analytics.stress.unquantifiableFixFactor'
        )}
      </p>
      {#if impact.positions.length}
        <details>
          <summary>{$t('portfolios.analytics.stress.measuredRows', { n: impact.positions.length })}</summary>
          <div class="pa-scroll">
            <table class="pa-table">
              <thead>
                <tr>
                  <th>{$t('portfolios.analytics.stress.position')}</th>
                  <th class="r">{$t('portfolios.analytics.stress.weight')}</th>
                  <th class="r">{$t('portfolios.analytics.stress.move')}</th>
                </tr>
              </thead>
              <tbody>
                {#each impact.positions as p (p.symbol)}
                  <tr>
                    <td>{p.symbol}</td>
                    <td class="r pa-muted">{fmtPct(p.weight_pct)}</td>
                    <td class="r" class:pa-down={p.move_pct < 0} class:pa-up={p.move_pct > 0}
                      >{fmtSignedPct(p.move_pct)}</td
                    >
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        </details>
      {/if}
    </div>
  {:else if impact}
    <div class="result">
      <div class="headline">
        <span class="lbl">{result.scenario.name}</span>
        <span class="big"><Delta value={impact.impact_pct} size="md" empty={na()} /></span>
        <span class="money">{fmtSignedMoney(impact.impact_value, currency)}</span>
      </div>
      <!-- Coverage before the number, never after it: a -11.8% measured over 74% of the
           book is a different statement from a -11.8% measured over all of it. -->
      <p class="coverage" class:partial>
        {$t('portfolios.analytics.stress.coverage', { pct: fmtRatioPct(impact.coverage, 0) })}
        {#if impact.unexplained.length}
          <span class="unexplained">
            {$t('portfolios.analytics.stress.unexplained', { names })}
          </span>
        {/if}
      </p>
      <p class="after">
        {$t('portfolios.analytics.stress.after', {
          value: fmtMoney(impact.resulting_net_worth, currency)
        })}
      </p>
      {#if impact.missing_factors.length}
        <p class="warn">
          {$t('portfolios.analytics.stress.missingFactors', { names: impact.missing_factors.join(', ') })}
        </p>
      {/if}
    </div>

    {#if impact.by_factor.length}
      <div class="factors">
        {#each impact.by_factor as f (f.factor)}
          <div class="fac">
            <span>{$t(`portfolios.analytics.stress.factor.${f.factor}`)}</span>
            <Delta value={f.impact_pct} empty={na()} />
          </div>
        {/each}
      </div>
    {/if}

    {#if impact.positions.length}
      <div class="pa-scroll">
        <table class="pa-table">
          <thead>
            <tr>
              <th>{$t('portfolios.analytics.stress.position')}</th>
              <th class="r">{$t('portfolios.analytics.stress.weight')}</th>
              <th class="r">{$t('portfolios.analytics.stress.move')}</th>
              {#if impact.positions[0].trough_pct != null}
                <th class="r">{$t('portfolios.analytics.stress.trough')}</th>
              {/if}
              <th class="r">{$t('portfolios.analytics.stress.impact')}</th>
            </tr>
          </thead>
          <tbody>
            {#each impact.positions as p (p.symbol)}
              <tr>
                <td>{p.symbol}</td>
                <td class="r pa-muted">{fmtPct(p.weight_pct)}</td>
                <td class="r" class:pa-down={p.move_pct < 0} class:pa-up={p.move_pct > 0}>{fmtSignedPct(p.move_pct)}</td>
                {#if p.trough_pct != null}
                  <td class="r pa-down">{fmtSignedPct(p.trough_pct)}</td>
                {/if}
                <td class="r" class:pa-down={p.impact < 0} class:pa-up={p.impact > 0}>{fmtSignedMoney(p.impact, currency)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  {:else}
    <div class="ready">
      <h4>{$t('portfolios.analytics.stress.readiness')}</h4>
      <div class="chips">
        {#each Object.entries(d.factors) as [id, proxy] (id)}
          <span class="chip ok">{$t(`portfolios.analytics.stress.factor.${id}`)} · {proxy}</span>
        {/each}
        {#each d.factors_missing as id (id)}
          <span class="chip miss">{$t(`portfolios.analytics.stress.factor.${id}`)} · {$t('portfolios.analytics.stress.noProxy')}</span>
        {/each}
      </div>
      {#if d.unexplained.length}
        <p class="warn">
          {$t('portfolios.analytics.stress.unexplainedReady', {
            names: d.unexplained.map((u) => u.symbol).join(', ')
          })}
        </p>
      {/if}
      {#if d.uncovered.length}
        <p class="warn">{$t('portfolios.analytics.stress.uncovered', { names: d.uncovered.join(', ') })}</p>
      {/if}
      {#if d.cash_pct != null}
        <p class="hint">{$t('portfolios.analytics.stress.cash', { pct: fmtPct(d.cash_pct) })}</p>
      {/if}
      {#if d.betas.length}
        <details>
          <summary>{$t('portfolios.analytics.stress.betas', { n: d.betas.length })}</summary>
          <div class="pa-scroll">
            <table class="pa-table">
              <thead>
                <tr>
                  <th>{$t('portfolios.analytics.stress.position')}</th>
                  <th>{$t('portfolios.analytics.stress.factorCol')}</th>
                  <th class="r">β</th>
                  <th class="r">R²</th>
                  <th class="r">{$t('portfolios.analytics.stress.rows')}</th>
                </tr>
              </thead>
              <tbody>
                {#each d.betas as b (b.asset_id + b.factor)}
                  <tr>
                    <td>{b.symbol}</td>
                    <td>{$t(`portfolios.analytics.stress.factor.${b.factor}`)}</td>
                    <td class="r">{fmtFixed(b.beta, 2)}</td>
                    <td class="r pa-muted">{fmtFixed(b.r2, 2)}</td>
                    <td class="r pa-muted">{b.rows}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        </details>
      {:else}
        <p class="hint">{$t('portfolios.analytics.stress.noBetas')}</p>
      {/if}
    </div>
  {/if}

  <Sample sample={block.sample} />
  </div>
{/if}

<style>
  .scenarios {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  /* The card owns the frame; the run and edit buttons inside it are bare. */
  .scen {
    display: flex;
    align-items: flex-start;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    background: var(--surface);
    color: var(--text);
    transition:
      border-color 160ms ease,
      background-color 160ms ease;
  }
  .scen:hover:has(.run:not(:disabled)) {
    border-color: var(--accent);
  }
  .run {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: var(--space-2) 0 var(--space-2) var(--space-3);
    border: 0;
    background: transparent;
    color: inherit;
    cursor: pointer;
    text-align: left;
  }
  .edit {
    padding: var(--space-2);
    border: 0;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .edit:hover {
    color: var(--text);
  }
  .scen.add {
    align-items: center;
    align-self: stretch;
    gap: var(--space-1);
    padding: var(--space-2) var(--space-3);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    font-size: 12px;
  }
  .scen.add:hover {
    color: var(--text);
  }
  .scen.active {
    border-color: var(--accent);
    background: var(--accent-soft);
  }
  .scen .name {
    font-weight: 600;
    font-size: 13px;
  }
  .scen .kind {
    font-size: 11px;
    color: var(--dim);
  }
  /* The result is one card: the scenario as the eyebrow, the impact as the headline. */
  .result,
  .ready {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-4);
    background: var(--surface);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius-lg);
  }
  .headline {
    display: flex;
    align-items: baseline;
    gap: var(--space-2) var(--space-4);
    flex-wrap: wrap;
  }
  .headline .lbl {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--dim);
    width: 100%;
  }
  .result .big :global(.delta) {
    font-size: 28px;
    font-family: var(--font);
    font-weight: 600;
  }
  .money {
    font-family: var(--mono);
    font-size: 15px;
    font-variant-numeric: tabular-nums;
    color: var(--muted);
  }
  .coverage,
  .after,
  .hint {
    margin: 0;
    font-size: 12px;
    color: var(--dim);
  }
  .coverage.partial {
    color: var(--amber-ink);
    font-weight: 600;
  }
  .unexplained {
    display: block;
    font-weight: 400;
  }
  .warn {
    margin: 0;
    font-size: 12px;
    color: var(--amber-ink);
  }
  .err {
    margin: 0;
    color: var(--red);
    font-size: 12px;
  }
  .factors {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .fac {
    display: flex;
    gap: var(--space-2);
    align-items: baseline;
    padding: var(--space-2) var(--space-3);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    font-size: 12px;
  }
  h4 {
    margin: 0;
    font-size: 13px;
    font-weight: 600;
    color: var(--text);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  /* Readiness tags: hairline; a factor with no proxy reads dim and says so in words. */
  .chip {
    padding: 1px var(--space-2);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius-sm);
    font-size: 12px;
  }
  .chip.ok {
    color: var(--text);
  }
  .chip.miss {
    color: var(--dim);
  }
  details {
    margin-top: var(--space-1);
  }
  summary {
    cursor: pointer;
    font-size: 12px;
    color: var(--dim);
  }
  summary:hover {
    color: var(--text);
  }
  .spin {
    color: var(--muted);
  }
  @media (max-width: 767px) {
    .run,
    .scen.add {
      min-height: 44px;
    }
    .edit {
      display: inline-flex;
      align-items: center;
      justify-content: center;
      align-self: stretch;
      min-width: 44px;
      min-height: 44px;
    }
    .scen .name {
      overflow-wrap: anywhere;
    }
    summary {
      padding: var(--space-3) 0;
    }
  }
</style>
