<script>
  // Exit-signal lab: the selected closed trades replayed under simple SL/TP/exit rules.
  // Entry, size and fees stay the trader's own, only the exit moves, so every figure here
  // is the price of the exit. The server does the replay (/api/journal/exits/simulate);
  // this screen ranks, compares and drills down, and can hand the tuning to the agent.
  import { untrack } from 'svelte';
  import { goto } from '$app/navigation';
  import { journalApi, fmtMoney, fmtSignedMoney, fmtPct, fmtNum } from './api.js';
  import { agentApi } from '$lib/modules/agent/api.js';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import { t, locale } from '$lib/i18n';
  import Button from '$lib/ui/Button.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import ExitCurves from './ExitCurves.svelte';
  import ExitChart from './ExitChart.svelte';

  // `selected` is owned by the page so leaving the lab and coming back keeps the pick;
  // null means "not picked yet", which defaults to every replayable trade.
  let { categoryId = '', selected = $bindable(null), onback } = $props();

  const MAX_COMPARE = 6;
  const MAX_TRADES = 500;

  // ── Trade picker ──
  let pool = $state(null); // { closed, trades } from /exits/ready
  let poolErr = $state('');
  let pickerOpen = $state(true);
  let q = $state('');
  $effect(() => {
    const scope = { category_id: categoryId || undefined };
    journalApi
      .exitsReady(scope)
      .then((r) => {
        pool = r;
        const ids = new Set(r.trades.map((x) => x.id));
        // Drop what left the scope since the last visit.
        const prev = untrack(() => selected);
        selected = prev ? prev.filter((id) => ids.has(id)) : r.trades.map((x) => x.id);
      })
      .catch((e) => (poolErr = e.message));
  });
  const picked = $derived(new Set(selected ?? []));
  const shown = $derived.by(() => {
    const s = q.trim().toLowerCase();
    return (pool?.trades ?? []).filter((x) => !s || x.ticker.toLowerCase().includes(s));
  });
  const allShown = $derived(shown.length > 0 && shown.every((x) => picked.has(x.id)));
  function toggleTrade(id) {
    selected = picked.has(id) ? selected.filter((x) => x !== id) : [...(selected ?? []), id];
  }
  function toggleShown() {
    const ids = new Set(shown.map((x) => x.id));
    selected = allShown
      ? selected.filter((id) => !ids.has(id))
      : [...new Set([...(selected ?? []), ...ids])];
  }

  let report = $state(null);
  let busy = $state(false);
  let err = $state('');
  let mode = $state('overlay');
  let extend = $state(20);
  let objective = $state('net');

  // Signals ticked for the curves and the chart, the one opened below the table, and the
  // trade drawn on its candles.
  let compare = $state([]);
  let focus = $state(null);
  let focusTrade = $state(null);
  let bars = $state(null);
  let barsErr = $state('');

  const colors = $derived(chartColors());
  const OBJECTIVES = ['net', 'drawdown', 'win_rate', 'avg_r', 'profit_factor'];

  async function run() {
    if (busy || !selected?.length) return;
    busy = true;
    err = '';
    try {
      report = await journalApi.exitsSimulate({
        trade_ids: selected,
        mode,
        extend_bars: Number(extend) || 0,
        objective
      });
      const b = report.best;
      compare = [b.sl, b.tp, b.combo].filter(Boolean);
      focus = b.combo ?? b.tp ?? b.sl ?? report.results[0]?.id ?? null;
      focusTrade = null;
      pickerOpen = false;
    } catch (e) {
      err = e.message;
    } finally {
      busy = false;
    }
  }

  // ── Labels ──
  const byId = $derived(new Map((report?.results ?? []).map((r) => [r.id, r])));
  function label(r) {
    if (!r) return '';
    if (r.rule === 'combo') {
      const b = report.best;
      return `${label(byId.get(b.sl))} + ${label(byId.get(b.tp))}`;
    }
    const s = $t(`journal.exits.rule.${r.rule}`, r.params);
    return s.startsWith('journal.') ? r.id : s;
  }
  const roleLabel = (r) => (r.rule === 'combo' ? 'SL+TP' : $t(`journal.exits.role.${r.role}`));

  // ── Table ──
  let sortKey = $state('');
  let sortDir = $state(-1);
  const COLS = [
    { key: 'net', label: 'journal.exits.col.net', money: true },
    { key: 'delta', label: 'journal.exits.col.delta', money: true, signed: true },
    { key: 'win_rate', label: 'journal.exits.col.winRate', pct: true },
    { key: 'avg_r', label: 'journal.exits.col.avgR' },
    { key: 'profit_factor', label: 'journal.exits.col.pf' },
    { key: 'max_drawdown', label: 'journal.exits.col.maxDd', money: true },
    { key: 'improved', label: 'journal.exits.col.improved' },
    { key: 'avg_hold_bars', label: 'journal.exits.col.hold' }
  ];
  function setSort(k) {
    if (sortKey === k) sortDir = -sortDir;
    else {
      sortKey = k;
      sortDir = k === 'max_drawdown' || k === 'avg_hold_bars' ? 1 : -1;
    }
  }
  const rows = $derived.by(() => {
    const list = [...(report?.results ?? [])];
    if (!sortKey) return list;
    const v = (r) => r.stats[sortKey] ?? -Infinity;
    return list.sort((a, b) => (v(a) - v(b)) * sortDir);
  });
  function cell(r, c) {
    const v = r.stats[c.key];
    if (v == null) return '—';
    if (c.money) return c.signed ? fmtSignedMoney(v, report.currency) : fmtMoney(v, report.currency);
    if (c.pct) return fmtPct(v, 1);
    if (c.key === 'improved') return `${r.stats.improved} / ${r.stats.worsened}`;
    return fmtNum(v, c.key === 'avg_hold_bars' ? 1 : 2);
  }
  function toggleCompare(id) {
    if (compare.includes(id)) compare = compare.filter((x) => x !== id);
    else if (compare.length < MAX_COMPARE) compare = [...compare, id];
  }
  const colorOf = (id) => colors.series[compare.indexOf(id) % colors.series.length] ?? colors.accent;

  const curves = $derived(
    compare
      .map((id) => byId.get(id))
      .filter(Boolean)
      .map((r) => ({ label: label(r), color: colorOf(r.id), curve: r.stats.curve }))
  );

  // ── Drill-down ──
  const focused = $derived(focus ? byId.get(focus) : null);
  const tradeById = $derived(new Map((report?.trades ?? []).map((x) => [x.id, x])));
  const focusRows = $derived(
    (focused?.per_trade ?? []).map((s) => ({ sim: s, trade: tradeById.get(s.trade_id) }))
  );
  async function openTrade(id) {
    focusTrade = id;
    bars = null;
    barsErr = '';
    try {
      bars = await journalApi.exitsBars(id, report.extend_bars);
    } catch (e) {
      barsErr = e.message;
    }
  }
  const chartExits = $derived.by(() => {
    if (!focusTrade || !report) return [];
    const ids = [...new Set([...(focus ? [focus] : []), ...compare])];
    return ids
      .map((id) => byId.get(id))
      .filter(Boolean)
      .map((r) => {
        const s = r.per_trade?.find((x) => x.trade_id === focusTrade);
        if (!s || s.outcome === 'unchanged' || s.outcome === 'not_applicable') return null;
        return {
          label: label(r),
          color: compare.includes(r.id) ? colorOf(r.id) : colors.accent,
          exit_bar: s.exit_bar,
          exit_price: s.exit_price
        };
      })
      .filter(Boolean);
  });
  const fmtDate = (iso) => (iso ? new Date(iso).toLocaleDateString() : '—');

  // ── Agent ──
  let agentOpen = $state(false);
  let agentGoal = $state('net');
  let agentNotes = $state('');
  let agentPick = $state([]);
  let agentBusy = $state(false);
  let agentErr = $state('');
  let agentNoTools = $state(false);
  async function openAgent() {
    agentGoal = objective;
    agentPick = (report?.results ?? [])
      .filter((r) => r.rule !== 'combo')
      .slice(0, 5)
      .map((r) => r.id);
    agentErr = '';
    agentOpen = true;
    try {
      const a = await agentApi.getAgent();
      agentNoTools = !a?.mcp_token_id;
    } catch {
      agentNoTools = false;
    }
  }
  function agentPrompt() {
    const lang = { fr: 'French', de: 'German', es: 'Spanish', it: 'Italian', pt: 'Portuguese', zh: 'Chinese' }[$locale] ?? 'English';
    const cur = report.currency;
    const b = report.baseline;
    const line = (r) =>
      `- ${r.id} (${r.rule} ${JSON.stringify(r.params)}): net ${r.stats.net.toFixed(2)}, delta ${r.stats.delta.toFixed(2)}, win ${r.stats.win_rate.toFixed(1)}%, avgR ${r.stats.avg_r?.toFixed(2) ?? 'n/a'}, PF ${r.stats.profit_factor?.toFixed(2) ?? 'n/a'}, maxDD ${r.stats.max_drawdown.toFixed(2)}, fired ${r.stats.earlier + r.stats.later}/${r.stats.trades}`;
    const picked = agentPick.map((id) => byId.get(id)).filter(Boolean);
    return [
      `Tune the exit rules of ${report.trades.length} closed Trading Journal trades with the exit-signal lab.`,
      `Tools: GET /api/journal/exits/catalog (rules and allowed parameter ranges), POST /api/journal/exits/simulate (read-only replay). Always pass view="summary".`,
      `Fixed inputs: mode="${report.mode}", extend_bars=${report.extend_bars}, trade_ids=${JSON.stringify(report.trades.map((x) => x.id))}`,
      `Objective: ${agentGoal}.${agentNotes.trim() ? ` Constraints from the trader: ${agentNotes.trim()}` : ''}`,
      `Real exits (baseline, ${cur}): net ${b.net.toFixed(2)}, win ${b.win_rate.toFixed(1)}%, avgR ${b.avg_r?.toFixed(2) ?? 'n/a'}, PF ${b.profit_factor?.toFixed(2) ?? 'n/a'}, maxDD ${b.max_drawdown.toFixed(2)}.`,
      `First run, the signals to start from:`,
      ...picked.map(line),
      `Method: vary their parameters inside the catalog ranges (up to 60 signals per call), refine around the best values, then test the best stop together with the best target or exit (combo=true). Do not write anything anywhere.`,
      `Report: the recommended stop and target/exit with their parameters, their figures against the baseline, the trade count, and whether neighbouring parameters give a similar result. If they do not, or if there are few trades, say the gain is likely noise.`,
      `Reply in ${lang}.`
    ].join('\n');
  }
  async function sendAgent() {
    agentBusy = true;
    agentErr = '';
    try {
      const conv = await agentApi.createConversation();
      sessionStorage.setItem(
        'otw-agent-handoff',
        JSON.stringify({ conversationId: conv.id, message: agentPrompt() })
      );
      goto('/agent');
    } catch (e) {
      agentErr = e.message;
      agentBusy = false;
    }
  }
</script>

<div class="lab">
  <div class="toolbar">
    <Button icon="chevron-left" onclick={onback}>{$t('journal.exits.back')}</Button>
    <span class="count">
      {$t('journal.exits.pickedCount', { n: selected?.length ?? 0, total: pool?.trades.length ?? 0 })}
    </span>
    <div class="controls">
      <Dropdown
        bind:value={mode}
        title={$t('journal.exits.mode.title')}
        ariaLabel={$t('journal.exits.mode.title')}
        options={[
          { value: 'overlay', label: $t('journal.exits.mode.overlay') },
          { value: 'replace', label: $t('journal.exits.mode.replace') }
        ]}
      />
      {#if mode === 'replace'}
        <label class="ext" title={$t('journal.exits.extendHint')}>
          {$t('journal.exits.extend')}
          <input type="number" min="0" max="500" bind:value={extend} />
        </label>
      {/if}
      <Dropdown
        bind:value={objective}
        title={$t('journal.exits.objective')}
        ariaLabel={$t('journal.exits.objective')}
        options={OBJECTIVES.map((o) => ({ value: o, label: $t(`journal.exits.obj.${o}`) }))}
      />
      <Button
        variant="primary"
        icon="play"
        loading={busy}
        disabled={!selected?.length || selected.length > MAX_TRADES}
        onclick={run}
      >
        {$t('journal.exits.run')}
      </Button>
      {#if report}
        <Button icon="sparkles" onclick={openAgent}>{$t('journal.exits.agent.button')}</Button>
      {/if}
    </div>
  </div>
  <p class="mode-hint">{$t(`journal.exits.mode.${mode}Hint`)}</p>

  <ErrorText error={err} />
  <ErrorText error={poolErr} />

  <section>
    <h3>
      <button class="fold" onclick={() => (pickerOpen = !pickerOpen)}>
        {pickerOpen ? '▾' : '▸'} {$t('journal.exits.picker.title')}
      </button>
      {#if (selected?.length ?? 0) > MAX_TRADES}
        <span class="warn">{$t('journal.exits.hint.tooMany')}</span>
      {/if}
    </h3>
    {#if pickerOpen}
      {#if !pool}
        {#if !poolErr}<Skeleton rows={4} height="var(--row-h)" gap="1px" />{/if}
      {:else if !pool.trades.length}
        <p class="muted">{$t('journal.exits.hint.noData')}</p>
      {:else}
        <div class="picker-bar">
          <input placeholder={$t('journal.trades.filter.ticker')} bind:value={q} />
          <span class="muted">{$t('journal.exits.picker.scope', { ready: pool.trades.length, closed: pool.closed })}</span>
        </div>
        <div class="table-wrap picker">
          <table class="tbl">
            <thead>
              <tr>
                <th class="pick">
                  <input
                    type="checkbox"
                    checked={allShown}
                    aria-label={$t('journal.exits.selectAll')}
                    title={$t('journal.exits.selectAll')}
                    onchange={toggleShown}
                  />
                </th>
                <th>{$t('journal.trades.col.ticker')}</th>
                <th>{$t('journal.trades.col.side')}</th>
                <th>{$t('journal.trades.col.entry')}</th>
                <th>{$t('journal.trades.col.exit')}</th>
                <th class="num">{$t('journal.trades.col.netPnl')}</th>
              </tr>
            </thead>
            <tbody>
              {#each shown as x (x.id)}
                <tr class:focus={picked.has(x.id)} onclick={() => toggleTrade(x.id)}>
                  <td class="pick">
                    <input type="checkbox" checked={picked.has(x.id)} aria-label={x.ticker} tabindex="-1" />
                  </td>
                  <td class="strong">{x.ticker}</td>
                  <td><span class="side {x.side}">{x.side}</span></td>
                  <td class="mono">{fmtDate(x.entry_at)}</td>
                  <td class="mono">{fmtDate(x.exit_at)}</td>
                  <td class="num" class:pos={x.net_pnl > 0} class:neg={x.net_pnl < 0}>
                    {fmtSignedMoney(x.net_pnl, x.currency)}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    {/if}
  </section>

  {#if busy && !report}
    <Skeleton rows={8} height="var(--row-h)" gap="1px" />
  {:else if report}
    {#if report.skipped.length}
      <p class="skipped">
        {$t('journal.exits.skipped', { n: report.skipped.length })}
        {#each Object.entries(report.skipped.reduce((m, s) => ((m[s.reason] = (m[s.reason] ?? 0) + 1), m), {})) as [reason, n]}
          <span class="reason">{$t(`journal.exits.reason.${reason}`)} ({n})</span>
        {/each}
      </p>
    {/if}

    {#if !report.trades.length}
      <p class="muted">{$t('journal.exits.nothing')}</p>
    {:else}
      <!-- The verdict first: the real exits against the best stop, target and pair. -->
      <div class="cards">
        <div class="card">
          <span class="k">{$t('journal.exits.baseline')}</span>
          <span class="v">{fmtMoney(report.baseline.net, report.currency)}</span>
          <span class="s">
            {fmtPct(report.baseline.win_rate, 0)} · DD {fmtMoney(report.baseline.max_drawdown, report.currency)}
          </span>
        </div>
        {#each [['sl', report.best.sl], ['tp', report.best.tp], ['combo', report.best.combo]] as [k, id]}
          {@const r = id ? byId.get(id) : null}
          <div class="card">
            <span class="k">{$t(`journal.exits.best.${k}`)}</span>
            {#if r}
              <span class="name">{label(r)}</span>
              <span class="v" class:pos={r.stats.delta > 0} class:neg={r.stats.delta < 0}>
                {fmtSignedMoney(r.stats.delta, report.currency)}
              </span>
              <span class="s">
                {fmtPct(r.stats.win_rate, 0)} · DD {fmtMoney(r.stats.max_drawdown, report.currency)}
              </span>
            {:else}
              <span class="s">{$t('journal.exits.best.none')}</span>
            {/if}
          </div>
        {/each}
      </div>
      {#if report.trades.length < 20}
        <p class="warn">{$t('journal.exits.fewTrades', { n: report.trades.length })}</p>
      {/if}

      <section>
        <h3>{$t('journal.exits.curves')}</h3>
        <ExitCurves baseline={report.baseline.curve} series={curves} currency={report.currency} />
      </section>

      <section>
        <h3>{$t('journal.exits.table')} <span class="sub">{$t('journal.exits.tableHint', { n: MAX_COMPARE })}</span></h3>
        <div class="table-wrap">
          <table class="tbl">
            <thead>
              <tr>
                <th class="pick"></th>
                <th>{$t('journal.exits.col.signal')}</th>
                <th>{$t('journal.exits.col.role')}</th>
                <th class="num" title={$t('journal.exits.col.firedHint')}>{$t('journal.exits.col.fired')}</th>
                {#each COLS as c}
                  <th class="num sortable" onclick={() => setSort(c.key)}>
                    {$t(c.label)}{sortKey === c.key ? (sortDir > 0 ? ' ▲' : ' ▼') : ''}
                  </th>
                {/each}
              </tr>
            </thead>
            <tbody>
              <tr class="base">
                <td></td>
                <td class="strong">{$t('journal.exits.baseline')}</td>
                <td></td>
                <td class="num">—</td>
                {#each COLS as c}
                  <td class="num">{c.key === 'delta' ? '—' : cell({ stats: report.baseline }, c)}</td>
                {/each}
              </tr>
              {#each rows as r (r.id)}
                <tr class:focus={focus === r.id} onclick={() => (focus = r.id)}>
                  <td class="pick" onclick={(e) => e.stopPropagation()}>
                    <input
                      type="checkbox"
                      checked={compare.includes(r.id)}
                      disabled={!compare.includes(r.id) && compare.length >= MAX_COMPARE}
                      aria-label={label(r)}
                      onchange={() => toggleCompare(r.id)}
                    />
                    {#if compare.includes(r.id)}<span class="dot" style:background={colorOf(r.id)}></span>{/if}
                  </td>
                  <td class="strong">{label(r)}</td>
                  <td><span class="role {r.role}">{roleLabel(r)}</span></td>
                  <td class="num" title={$t('journal.exits.firedDetail', r.stats)}>
                    {r.stats.earlier + r.stats.later}/{r.stats.trades}
                  </td>
                  {#each COLS as c}
                    <td
                      class="num"
                      class:pos={c.key === 'delta' && r.stats.delta > 0}
                      class:neg={c.key === 'delta' && r.stats.delta < 0}
                    >
                      {cell(r, c)}
                    </td>
                  {/each}
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      </section>

      {#if focused}
        <section>
          <h3>{$t('journal.exits.detail', { name: label(focused) })}</h3>
          <div class="table-wrap">
            <table class="tbl">
              <thead>
                <tr>
                  <th>{$t('journal.trades.col.ticker')}</th>
                  <th>{$t('journal.trades.col.side')}</th>
                  <th>{$t('journal.trades.col.entry')}</th>
                  <th class="num">{$t('journal.exits.col.real')}</th>
                  <th class="num">{$t('journal.exits.col.sim')}</th>
                  <th class="num">{$t('journal.exits.col.delta')}</th>
                  <th>{$t('journal.exits.col.outcome')}</th>
                </tr>
              </thead>
              <tbody>
                {#each focusRows as { sim, trade } (sim.trade_id)}
                  <tr class:focus={focusTrade === sim.trade_id} onclick={() => openTrade(sim.trade_id)}>
                    <td class="strong">{trade?.ticker}</td>
                    <td><span class="side {trade?.side}">{trade?.side}</span></td>
                    <td class="mono">{fmtDate(trade?.entry_at)}</td>
                    <td class="num">{fmtSignedMoney(trade?.net, report.currency)}</td>
                    <td class="num">{fmtSignedMoney(sim.net, report.currency)}</td>
                    <td class="num" class:pos={sim.net - trade?.net > 1e-9} class:neg={sim.net - trade?.net < -1e-9}>
                      {fmtSignedMoney(sim.net - (trade?.net ?? 0), report.currency)}
                    </td>
                    <td><span class="outcome {sim.outcome}">{$t(`journal.exits.outcome.${sim.outcome}`)}</span></td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        </section>
      {/if}

      {#if focusTrade}
        <section>
          <h3>
            {$t('journal.exits.chart.title', { ticker: tradeById.get(focusTrade)?.ticker ?? '' })}
            <button class="close" onclick={() => (focusTrade = null)} aria-label={$t('common.close')}>×</button>
          </h3>
          <ErrorText error={barsErr} />
          {#if bars}
            <ExitChart {bars} exits={chartExits} />
          {:else if !barsErr}
            <Skeleton rows={1} height="380px" />
          {/if}
        </section>
      {/if}
    {/if}
  {/if}
</div>

<Modal bind:open={agentOpen} size="md" title={$t('journal.exits.agent.title')}>
  <div class="agent">
    <p class="muted">{$t('journal.exits.agent.intro')}</p>
    {#if agentNoTools}<p class="warn">{$t('journal.exits.agent.noTools')}</p>{/if}
    <label class="field">
      <span>{$t('journal.exits.objective')}</span>
      <Dropdown
        bind:value={agentGoal}
        ariaLabel={$t('journal.exits.objective')}
        options={OBJECTIVES.map((o) => ({ value: o, label: $t(`journal.exits.obj.${o}`) }))}
      />
    </label>
    <label class="field">
      <span>{$t('journal.exits.agent.notes')}</span>
      <textarea rows="2" placeholder={$t('journal.exits.agent.notesPlaceholder')} bind:value={agentNotes}></textarea>
    </label>
    <div class="field">
      <span>{$t('journal.exits.agent.signals')}</span>
      <div class="picks">
        {#each (report?.results ?? []).filter((r) => r.rule !== 'combo') as r (r.id)}
          <label class="pickrow">
            <input
              type="checkbox"
              checked={agentPick.includes(r.id)}
              onchange={() =>
                (agentPick = agentPick.includes(r.id)
                  ? agentPick.filter((x) => x !== r.id)
                  : [...agentPick, r.id])}
            />
            {label(r)}
          </label>
        {/each}
      </div>
    </div>
    <ErrorText error={agentErr} />
  </div>
  {#snippet footer()}
    <Button onclick={() => (agentOpen = false)}>{$t('common.cancel')}</Button>
    <Button variant="primary" icon="sparkles" loading={agentBusy} disabled={!agentPick.length} onclick={sendAgent}>
      {$t('journal.exits.agent.send')}
    </Button>
  {/snippet}
</Modal>

<style>
  .lab {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  .count {
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .controls {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
    margin-left: auto;
  }
  .fold {
    background: none;
    border: none;
    padding: 0;
    color: var(--text);
    font: inherit;
    font-weight: var(--fw-medium);
    cursor: pointer;
  }
  .picker-bar {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    margin-bottom: var(--space-2);
  }
  .picker-bar input {
    width: 160px;
  }
  .picker {
    max-height: 320px;
    overflow-y: auto;
  }
  .ext {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .ext input {
    width: 70px;
  }
  .mode-hint,
  .muted {
    color: var(--muted);
    font-size: var(--text-sm);
    margin: 0;
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
  .warn {
    color: var(--amber);
    font-size: var(--text-sm);
    margin: 0;
  }
  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: var(--space-3);
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: var(--space-3);
    border: 0.5px solid var(--border);
  }
  .card .k {
    font-size: var(--text-xs);
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .card .name {
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
  }
  .card .v {
    font-family: var(--mono);
    font-size: 1.1rem;
    font-variant-numeric: tabular-nums;
  }
  .card .s {
    font-size: var(--text-xs);
    color: var(--muted);
    font-family: var(--mono);
  }
  section h3 {
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    margin: 0 0 var(--space-2);
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .sub {
    color: var(--muted);
    font-weight: normal;
  }
  .close {
    margin-left: auto;
    background: none;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: 1.1rem;
  }
  .table-wrap {
    overflow-x: auto;
    border: 0.5px solid var(--border);
  }
  tbody tr {
    cursor: pointer;
  }
  tbody tr:hover td {
    background: var(--surface-2);
  }
  tr.focus td {
    background: var(--surface-2);
  }
  tr.base td {
    cursor: default;
    color: var(--muted);
  }
  .sortable {
    cursor: pointer;
    white-space: nowrap;
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
  .pick {
    width: 44px;
    white-space: nowrap;
  }
  .dot {
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    margin-left: 4px;
    vertical-align: middle;
  }
  .role,
  .side,
  .outcome {
    text-transform: uppercase;
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    letter-spacing: 0.04em;
  }
  .role.sl,
  .side.short {
    color: var(--red);
  }
  .role.tp,
  .side.long {
    color: var(--green);
  }
  .role.exit {
    color: var(--accent);
  }
  .outcome.unchanged,
  .outcome.not_applicable {
    color: var(--muted);
  }
  .pos {
    color: var(--green);
  }
  .neg {
    color: var(--red);
  }
  .agent {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: var(--text-sm);
  }
  .picks {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: var(--space-1) var(--space-3);
    max-height: 220px;
    overflow-y: auto;
  }
  .pickrow {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
</style>
