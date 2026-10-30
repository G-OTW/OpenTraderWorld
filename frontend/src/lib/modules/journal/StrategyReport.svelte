<script>
  // A strategy's report card, in a modal: what the strategy earns per trade, where (hour,
  // weekday, regime, instrument), and four standard what-ifs run on its own trades. Each
  // what-if opens in the lab with its parameters filled.
  import { journalApi, fmtMoney, fmtSignedMoney, fmtPct, fmtNum } from './api.js';
  import { statementParams } from './lab.js';
  import { SEVERITY_ICON } from './insights.js';
  import { t, locale } from '$lib/i18n';
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import Icon from '$lib/ui/Icon.svelte';

  let { open = $bindable(false), strategy = null, categoryId = '', onopenlab = null } = $props();

  let card = $state(null);
  let busy = $state(false);
  let err = $state('');
  let cap = $state('');

  async function load() {
    if (!strategy) return;
    busy = true;
    err = '';
    try {
      const c = String(cap).trim();
      card = await journalApi.strategyReport(strategy.id, {
        category_id: categoryId || undefined,
        loss_cap: c === '' ? undefined : c
      });
    } catch (e) {
      err = e.message;
    } finally {
      busy = false;
    }
  }

  let loadedFor = null;
  $effect(() => {
    if (open && strategy && loadedFor !== strategy.id) {
      loadedFor = strategy.id;
      card = null;
      load();
    }
    if (!open) loadedFor = null;
  });

  const cur = $derived(card?.currency ?? 'USD');
  const s = $derived(card?.stats);
  const F = $derived({
    money: (v) => fmtSignedMoney(v, cur),
    int: (v) => String(Math.round(v)),
    factor: (v) => `×${fmtNum(v, 2)}`,
    pct: (v) => fmtPct(v, 1),
    num: (v) => fmtNum(v, 2)
  });
  const weekdayName = (d) =>
    new Date(Date.UTC(2024, 0, Number(d))).toLocaleDateString($locale, { weekday: 'short', timeZone: 'UTC' });
  const tables = $derived(
    s
      ? [
          { id: 'r', rows: s.r_dist.filter((b) => b.trades > 0), label: (k) => $t(`journal.lab.card.r.${k}`) },
          { id: 'hour', rows: s.by_hour, label: (k) => `${String(k).padStart(2, '0')}:00` },
          { id: 'weekday', rows: s.by_weekday, label: weekdayName },
          { id: 'regime', rows: s.by_regime, label: (k) => $t(`journal.market.regime.${k}`) },
          {
            id: 'instrument',
            rows: [...s.by_instrument].sort((a, b) => b.trades - a.trades).slice(0, 10),
            label: (k) => k
          }
        ]
      : []
  );
  const tiles = $derived(
    s
      ? [
          { k: 'trades', v: String(s.summary.n) },
          { k: 'net', v: fmtSignedMoney(s.summary.net, cur), tone: s.summary.net },
          { k: 'expectancy', v: s.summary.expectancy == null ? '-' : fmtSignedMoney(s.summary.expectancy, cur), tone: s.summary.expectancy },
          { k: 'winRate', v: s.summary.win_rate == null ? '-' : fmtPct(s.summary.win_rate, 0) },
          { k: 'avgR', v: s.avg_r == null ? '-' : `${fmtNum(s.avg_r, 2)} R`, sub: $t('journal.lab.card.rN', { n: s.r_n }) },
          { k: 'pf', v: s.summary.profit_factor == null ? '-' : fmtNum(s.summary.profit_factor, 2) },
          { k: 'maxDd', v: fmtMoney(s.summary.max_drawdown, cur) }
        ]
      : []
  );
  const scenarioLabel = (x) =>
    $t(`journal.lab.card.scenario.${x.key}${x.values?.cap != null ? 'Cap' : ''}`, statementParams(x.values, { ...F, money: (v) => fmtMoney(v, cur) }));
  const statementText = (st) => $t(`journal.lab.statement.${st.key}`, statementParams(st.values, F));
</script>

<Modal bind:open size="lg" title={$t('journal.lab.card.title', { name: strategy?.name ?? '' })}>
  <div class="card-body">
    <ErrorText error={err} />
    {#if busy && !card}
      <Skeleton rows={8} height="var(--row-h)" gap="1px" />
    {:else if card}
      {#if !s.summary.n}
        <p class="muted">{$t('journal.lab.card.empty')}</p>
      {:else}
        <div class="tiles">
          {#each tiles as x (x.k)}
            <div class="tile">
              <span class="k">{$t(`journal.lab.card.tile.${x.k}`)}</span>
              <span class="v" class:pos={x.tone > 0} class:neg={x.tone < 0}>{x.v}</span>
              {#if x.sub}<span class="s">{x.sub}</span>{/if}
            </div>
          {/each}
        </div>
        {#if s.summary.n < card.min_sample}
          <p class="warn">{$t('journal.lab.fewTrades', { n: s.summary.n, min: card.min_sample })}</p>
        {/if}

        <section>
          <h3>{$t('journal.lab.card.whatIf')}</h3>
          <div class="capbar">
            <label>
              {$t('journal.lab.size.cap')}
              <input type="number" min="0" step="any" placeholder={$t('journal.lab.size.capPlaceholder')} bind:value={cap} />
            </label>
            <Button icon="refresh-cw" loading={busy} onclick={load}>{$t('journal.lab.card.rerun')}</Button>
          </div>
          <div class="table-wrap">
            <table class="tbl">
              <thead>
                <tr>
                  <th>{$t('journal.lab.card.col.scenario')}</th>
                  <th class="num">{$t('journal.lab.metric.n')}</th>
                  <th class="num">{$t('journal.lab.col.before')}</th>
                  <th class="num">{$t('journal.lab.col.after')}</th>
                  <th class="num">{$t('journal.lab.col.delta')}</th>
                  <th class="num">{$t('journal.lab.card.col.ddDelta')}</th>
                  <th></th>
                </tr>
              </thead>
              <tbody>
                {#each card.scenarios as x (x.key)}
                  <tr>
                    <td>
                      <span class="strong">{scenarioLabel(x)}</span>
                      {#if x.missing}
                        <span class="sub muted">{$t(`journal.lab.card.missing.${x.missing}`)}</span>
                      {:else if x.statement}
                        <span class="sub statement {x.statement.severity}">
                          <Icon name={SEVERITY_ICON[x.statement.severity] ?? 'lightbulb'} size={12} />
                          {statementText(x.statement)}
                        </span>
                      {:else if x.n < card.min_sample}
                        <span class="sub muted">{$t('journal.lab.card.belowFloor', { min: card.min_sample })}</span>
                      {/if}
                    </td>
                    {#if x.missing}
                      <td class="num muted">-</td><td class="num muted">-</td><td class="num muted">-</td>
                      <td class="num muted">-</td><td class="num muted">-</td>
                    {:else}
                      <td class="num">{x.n}</td>
                      <td class="num">{fmtSignedMoney(x.before_net, cur)}</td>
                      <td class="num">{fmtSignedMoney(x.after_net, cur)}</td>
                      <td class="num" class:pos={x.delta_net > 1e-9} class:neg={x.delta_net < -1e-9}>
                        {fmtSignedMoney(x.delta_net, cur)}
                      </td>
                      <td class="num" class:pos={x.delta_drawdown < -1e-9} class:neg={x.delta_drawdown > 1e-9}>
                        {fmtSignedMoney(x.delta_drawdown, cur)}
                      </td>
                    {/if}
                    <td class="act">
                      {#if x.scenario && onopenlab}
                        <Button size="sm" icon="flask" onclick={() => onopenlab(x.scenario)}>
                          {$t('journal.lab.card.open')}
                        </Button>
                      {/if}
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
          {#if card.unmeasured.length}
            <p class="muted">{$t('journal.lab.unmeasured', { n: card.unmeasured.length })}</p>
          {/if}
        </section>

        <div class="grid">
          {#each tables as tb (tb.id)}
            {#if tb.rows.length}
              <section>
                <h3>{$t(`journal.lab.card.by.${tb.id}`)}</h3>
                <div class="table-wrap">
                  <table class="tbl">
                    <thead>
                      <tr>
                        <th></th>
                        <th class="num">{$t('journal.lab.metric.n')}</th>
                        <th class="num">{$t('journal.lab.metric.net')}</th>
                        <th class="num">{$t('journal.lab.metric.win_rate')}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {#each tb.rows as b (b.key)}
                        <tr>
                          <td>{tb.label(b.key)}</td>
                          <td class="num">{b.trades}</td>
                          <td class="num" class:pos={b.net > 0} class:neg={b.net < 0}>{fmtSignedMoney(b.net, cur)}</td>
                          <td class="num">{fmtPct(b.win_rate, 0)}</td>
                        </tr>
                      {/each}
                    </tbody>
                  </table>
                </div>
              </section>
            {/if}
          {/each}
        </div>
      {/if}
    {/if}
  </div>
</Modal>

<style>
  .card-body {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(110px, 1fr));
    gap: var(--space-2);
  }
  .tile {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: var(--space-2) var(--space-3);
    border: 0.5px solid var(--border);
  }
  .tile .k {
    font-size: var(--text-xs);
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .tile .v {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  .tile .s {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
    gap: var(--space-3);
  }
  section h3 {
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    margin: 0 0 var(--space-2);
  }
  .capbar {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    margin-bottom: var(--space-2);
  }
  .capbar label {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .capbar input {
    width: 120px;
  }
  .table-wrap {
    overflow-x: auto;
    border: 0.5px solid var(--border);
  }
  .num {
    text-align: right;
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .strong {
    font-weight: var(--fw-medium);
  }
  .sub {
    display: flex;
    align-items: flex-start;
    gap: var(--space-1);
    font-size: var(--text-xs);
    margin-top: 2px;
  }
  .statement :global(svg) {
    flex: none;
    margin-top: 1px;
  }
  .statement.info :global(svg) {
    color: var(--accent);
  }
  .statement.good :global(svg) {
    color: var(--green);
  }
  .act {
    text-align: right;
    white-space: nowrap;
  }
  .muted {
    color: var(--muted);
    font-size: var(--text-sm);
    margin: 0;
  }
  .warn {
    color: var(--amber);
    font-size: var(--text-sm);
    margin: 0;
  }
  .pos {
    color: var(--green);
  }
  .neg {
    color: var(--red);
  }
</style>
