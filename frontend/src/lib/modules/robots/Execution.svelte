<script>
  // The execution panel of a paper session that executes on a broker account.
  //
  // Two books side by side, the engine's (simulated) and the account's (real), and per trade
  // the simulated price next to the broker's: the point of the panel is that the two are never
  // mistaken for each other. What it shows is what the server's last sync read from the venue;
  // polling it calls no broker. Arming, disarming, adopting and Stop and flatten are the user's
  // acts, each one at a time.
  import { onMount } from 'svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Input from '$lib/ui/Input.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import BackgroundTradingModal from '$lib/brokers/BackgroundTradingModal.svelte';
  import { robotsApi } from './api.js';
  import { bookRows, compareRows, execState, nextStep, num, stateTone } from './execution.js';
  import { t } from '$lib/i18n';

  let { sessionId, onchanged = () => {} } = $props();
  const label = (key, params) => $t(`robots.exec.${key}`, params);
  const fmt = (v, digits = 6) => {
    const n = num(v);
    return n === null ? '-' : n.toLocaleString(undefined, { maximumFractionDigits: digits });
  };
  const when = (v) => (v && Number.isFinite(Date.parse(v)) ? new Date(v).toLocaleString() : '-');
  const LIMITS = ['max_order_qty', 'max_order_value', 'max_position_qty', 'max_position_value', 'collar_pct', 'max_orders_per_min', 'max_open_orders', 'max_loss', 'duplicate_secs'];

  let view = $state(null);
  let error = $state('');
  let busy = $state('');
  /** The act being confirmed: `arm`, `flatten` or `adopt`. */
  let confirming = $state('');
  let code = $state('');
  let needsCode = $state(false);
  let actError = $state('');
  let signOpen = $state(false);
  let alive = true;

  const state = $derived(execState(view));
  const tone = $derived(stateTone(state));
  const books = $derived(bookRows(view));
  const trades = $derived(compareRows(view));
  const level = $derived(view?.account?.level ?? 'missing');
  const limits = $derived(
    LIMITS.filter((k) => num(view?.limits?.[k]) !== null).map((k) => ({ key: k, value: k === 'max_loss' && view.limits.max_loss_pct ? `${view.limits[k]}%` : view.limits[k] }))
  );

  async function load() {
    try {
      const next = await robotsApi.execution(sessionId);
      if (!alive) return;
      view = next;
      error = '';
    } catch (e) {
      if (alive) error = e.message;
    }
  }

  onMount(() => {
    load();
    const timer = setInterval(() => {
      if (!document.hidden && !busy) load();
    }, 5000);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  });

  function ask(act) {
    confirming = act;
    actError = '';
    code = '';
    needsCode = false;
  }

  async function go() {
    const act = confirming;
    busy = act;
    actError = '';
    try {
      if (act === 'arm') await robotsApi.arm(sessionId, code);
      else if (act === 'flatten') await robotsApi.flatten(sessionId, code);
      else if (act === 'adopt') await robotsApi.adopt(sessionId);
      confirming = '';
      await load();
      onchanged();
    } catch (e) {
      const step = nextStep(e);
      if (step === 'code') needsCode = true;
      if (step === 'sign') {
        confirming = '';
        signOpen = true;
      }
      actError = e.message;
    } finally {
      busy = '';
    }
  }

  async function disarm() {
    busy = 'disarm';
    actError = '';
    try {
      await robotsApi.disarm(sessionId);
      await load();
      onchanged();
    } catch (e) {
      actError = e.message;
    } finally {
      busy = '';
    }
  }
</script>

{#if view?.target === 'broker'}
  <section class="panel exec" data-paper-execution>
    <div class="section-head">
      <div>
        <h3><Icon name="briefcase" size={14} /> {label('title')}</h3>
        <p>
          {view.account?.name ?? view.execution?.account ?? ''}
          <span class="level {level}">{label(`level.${level}`)}</span>
        </p>
      </div>
      <span class="state {tone}"><span class="dot"></span>{label(`state.${state}`)}</span>
    </div>

    {#if view.note}
      <div class="note {tone}" role="status"><Icon name={tone === 'bad' ? 'alert-triangle' : 'info'} size={14} /><span>{view.note}</span></div>
    {/if}
    {#if error}<div class="note bad" role="alert"><Icon name="alert-triangle" size={14} /><span>{error}</span></div>{/if}

    <div class="facts">
      <span><Icon name={view.armed_at ? 'zap' : 'pause'} size={13} />{view.armed_at ? label('armedSince', { when: when(view.armed_at) }) : label('notArmed')}</span>
      <span>{label('lastSync')} <b>{when(view.synced_at)}</b></span>
      <span>{label(`exits.${view.execution?.exits ?? 'watch'}`)}</span>
      {#if view.execution?.max_position_qty}<span>{label('maxPosition.short', { qty: view.execution.max_position_qty })}</span>{/if}
      <span class="limits" title={limits.map((l) => `${label(`limit.${l.key}`)} ${l.value}`).join('\n')}>
        {limits.length ? label('limitsSet', { n: limits.length }) : label('limitsNone')}
      </span>
    </div>

    <div class="actions">
      {#if view.armed_at}
        <Button size="sm" icon="pause" loading={busy === 'disarm'} disabled={!!busy} onclick={disarm}>{label('disarm')}</Button>
      {:else}
        <Button size="sm" variant="primary" icon="zap" disabled={!!busy || level === 'missing'} onclick={() => ask('arm')}>{label('arm')}</Button>
      {/if}
      {#if state === 'degraded'}
        <Button size="sm" disabled={!!busy} onclick={() => ask('adopt')}>{label('adopt')}</Button>
      {/if}
      <Button size="sm" variant="danger" icon="stop" disabled={!!busy || level === 'missing'} onclick={() => ask('flatten')}>{label('flatten')}</Button>
      {#if actError && !confirming}<span class="act-error">{actError}</span>{/if}
    </div>

    <!-- The two books. The engine's columns are what the simulation holds; the account's are
         what the venue reported at the last sync. -->
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div class="table-scroll" tabindex="0" role="region" aria-label={label('book')}>
      <table>
        <thead>
          <tr>
            <th>{label('instrument')}</th>
            {#each ['position', 'stop', 'target'] as k}
              <th class="sim" title={label('simulated')}><span class="src"><Icon name="flask" size={11} />{label(k)}</span></th>
            {/each}
            {#each ['held', 'own', 'drift', 'working'] as k, i}
              <th class="real" class:split={i === 0} title={label('atBroker')}><span class="src"><Icon name="briefcase" size={11} />{label(k)}</span></th>
            {/each}
          </tr>
        </thead>
        <tbody>
          {#each books as b (b.key)}
            <tr>
              <td><b>{b.ticker}</b>{#if b.symbol !== b.ticker}<small>{b.symbol}</small>{/if}</td>
              <td class="sim word">
                {b.side ? label(`side.${b.side}`) : label('flat')}
                {#if b.entry}<small>{label('entryAtClose', { side: label(`side.${b.entry.side}`) })}</small>{/if}
                {#if b.limits.length}<small>{label('limitsResting', { n: b.limits.length })}</small>{/if}
              </td>
              <td class="sim">{fmt(b.stop)}</td>
              <td class="sim">{fmt(b.target)}</td>
              <td class="real">{fmt(b.held)}</td>
              <td class="real">{fmt(b.own)}</td>
              <td class="real">
                {#if b.drift === null}-{:else}<span class="drift" class:off={b.drifting}>{b.drifting ? fmt(b.drift) : label('noDrift')}</span>{/if}
              </td>
              <td class="real">
                {#each b.working as w}<small class="order">{label(`role.${w.role}`)} {w.side === 'buy' ? label('buy') : label('sell')} {fmt(w.qty)}{#if w.limit ?? w.stop} @ {fmt(w.limit ?? w.stop)}{/if}</small>{:else}-{/each}
              </td>
            </tr>
          {:else}
            <tr><td colspan="8" class="empty-row">{label('notSynced')}</td></tr>
          {/each}
        </tbody>
      </table>
    </div>

    <!-- Each entry the session sent: the simulated price next to the broker's, and what the
         difference cost. -->
    <div class="sub-head"><h4>{label('trades')}</h4><span>{label('costHint')}</span></div>
    {#if trades.length}
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <div class="table-scroll" tabindex="0" role="region" aria-label={label('trades')}>
        <table>
          <thead>
            <tr class="groups">
              <th colspan="2"></th>
              <th colspan="3" class="sim-real">{label('entry')}</th>
              <th colspan="3" class="sim-real">{label('exit')}</th>
              <th></th>
            </tr>
            <tr>
              <th>{label('instrument')}</th>
              <th>{label('size')}</th>
              <th class="sim">{label('simulated')}</th>
              <th class="real">{label('atBroker')}</th>
              <th>{label('cost')}</th>
              <th class="sim">{label('simulated')}</th>
              <th class="real">{label('atBroker')}</th>
              <th>{label('cost')}</th>
              <th>{label('outcome')}</th>
            </tr>
          </thead>
          <tbody>
            {#each trades as r (r.client_id)}
              <tr>
                <td><b>{r.ticker}</b><small>{label(`side.${r.side}`)} · {when(r.at)}</small></td>
                <td>{fmt(r.filled ?? r.qty)}<small>{label('sizes', { engine: fmt(r.engine_qty), sent: fmt(r.qty) })}</small></td>
                <td class="sim">{fmt(r.sim_entry)}</td>
                <td class="real">{fmt(r.real_entry)}</td>
                <td class:worse={r.entrySlip > 0} class:better={r.entrySlip < 0}>{r.entrySlip === null ? '-' : fmt(r.entrySlip)}</td>
                <td class="sim">{fmt(r.sim_exit)}</td>
                <td class="real">{fmt(r.real_exit)}</td>
                <td class:worse={r.exitSlip > 0} class:better={r.exitSlip < 0}>{r.exitSlip === null ? '-' : fmt(r.exitSlip)}</td>
                <td title={r.reason ?? ''}><span class="outcome {r.outcome}">{label(`outcome.${r.outcome}`)}</span></td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {:else}
      <div class="no-trades"><b>{label('noTrades')}</b><span>{label('noTradesHint')}</span></div>
    {/if}
  </section>

  <Modal
    open={!!confirming}
    size="sm"
    title={confirming ? label(`${confirming}Title`) : ''}
    onclose={() => (confirming = '')}
  >
    <div class="confirm">
      {#if confirming === 'arm'}
        <p>{label('armBody', { account: view.account?.name ?? '' })}</p>
        {#if level === 'live'}<p class="live-warn"><Icon name="alert-triangle" size={13} /> {label('armBodyLive')}</p>{/if}
      {:else if confirming === 'flatten'}
        <p>{label('flattenBody')}</p>
      {:else if confirming === 'adopt'}
        <p>{label('adoptBody')}</p>
      {/if}
      {#if needsCode}
        <Input label={label('codeLabel')} bind:value={code} hint={label('codeHint')} autocomplete="one-time-code" inputmode="numeric" />
      {/if}
      {#if actError}<p class="act-error">{actError}</p>{/if}
    </div>
    {#snippet footer()}
      <Button variant="ghost" onclick={() => (confirming = '')}>{$t('common.cancel')}</Button>
      <Button variant={confirming === 'flatten' ? 'danger' : 'primary'} loading={!!busy} onclick={go}>
        {confirming ? label(confirming) : ''}
      </Button>
    {/snippet}
  </Modal>

  <BackgroundTradingModal bind:open={signOpen} account={view.account} onsigned={() => { signOpen = false; ask('arm'); }} />
{/if}

<style>
  /* Level-1 panel: square, one hairline, no lift. `panel`, not `card`: the global .card
     layer would add its radius and shadow. */
  .panel { border: var(--hairline) solid var(--border); background: var(--surface); overflow: hidden; min-width: 0; }
  .section-head { display: flex; align-items: flex-start; justify-content: space-between; gap: var(--space-3); padding: var(--space-4) var(--space-4) var(--space-2); }
  .section-head p { margin: var(--space-1) 0 0; color: var(--dim); font-size: var(--fs-desc); display: flex; align-items: center; gap: var(--space-2); flex-wrap: wrap; }
  h3 { font-size: var(--fs-item-title); font-weight: var(--fw-medium); letter-spacing: 0.02em; margin: 0; display: flex; align-items: center; gap: var(--space-2); }
  h4 { font-size: var(--fs-metric-label); font-weight: var(--fw-medium); text-transform: uppercase; letter-spacing: 0.06em; color: var(--dim); margin: 0; }
  /* Square hairline tags: the level and the outcome are codes, so they read in mono caps. */
  .level, .outcome { display: inline-flex; align-items: center; padding: 0 var(--space-1); border: var(--hairline) solid var(--border-control); font-family: var(--mono); font-size: var(--fs-metric-label); letter-spacing: 0.06em; text-transform: uppercase; color: var(--muted); }
  .level.live { color: var(--red); border-color: color-mix(in srgb, var(--red) 45%, transparent); font-weight: var(--fw-medium); }
  .level.paper { color: var(--green); border-color: color-mix(in srgb, var(--green) 45%, transparent); }
  /* Execution state: the dot carries the tone, the word stays in ink. */
  .state { display: inline-flex; gap: var(--space-2); align-items: center; padding: var(--space-1) var(--space-2); border: var(--hairline) solid var(--border-control); color: var(--text); font-size: var(--fs-metric-label); letter-spacing: 0.06em; text-transform: uppercase; white-space: nowrap; }
  .dot { display: inline-block; width: 6px; height: 6px; border-radius: 50%; background: var(--muted); flex: none; }
  .state.ok .dot { background: var(--green); }
  .state.warn .dot { background: var(--amber); }
  .state.bad .dot { background: var(--red); }
  /* Notes: a tint, a hairline in the hue, ink in --text; the icon carries the meaning. */
  .note { display: flex; align-items: flex-start; gap: var(--space-2); margin: var(--space-1) var(--space-4) var(--space-2); padding: var(--space-2) var(--space-3); border: var(--hairline) solid var(--border); background: var(--surface-2); color: var(--text); font-size: var(--fs-desc); line-height: var(--lh-base); }
  .note > :global(svg) { flex: none; color: var(--dim); }
  .note.warn { background: color-mix(in srgb, var(--amber) 14%, transparent); border-color: color-mix(in srgb, var(--amber) 45%, transparent); }
  .note.warn > :global(svg) { color: var(--amber); }
  .note.bad { background: color-mix(in srgb, var(--red) 10%, transparent); border-color: color-mix(in srgb, var(--red) 45%, transparent); }
  .note.bad > :global(svg) { color: var(--red); }
  .note span { flex: 1; overflow-wrap: anywhere; }
  .facts { display: flex; flex-wrap: wrap; gap: var(--space-2) var(--space-6); padding: var(--space-2) var(--space-4); color: var(--dim); font-size: var(--fs-desc); }
  .facts span { display: inline-flex; align-items: center; gap: var(--space-2); }
  .facts b { color: var(--text); font-family: var(--mono); font-weight: var(--fw-normal); }
  .limits { cursor: help; }
  .actions { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); padding: var(--space-2) var(--space-4) var(--space-3); border-bottom: var(--hairline) solid var(--border); }
  .act-error { color: var(--red); font-size: var(--fs-desc); margin: 0; }
  /* The two books: Table.svelte metrics (10px uppercase header, 36px rows, hairline rules,
     mono figures), kept raw for the grouped sim / real header row. */
  .table-scroll { max-height: 320px; overflow: auto; }
  table { border-collapse: collapse; width: 100%; font-size: var(--text-sm); white-space: nowrap; font-variant-numeric: tabular-nums; }
  th { height: var(--row-h); padding: 0 var(--space-3); color: var(--dim); font-size: var(--fs-metric-label); font-weight: var(--fw-medium); text-transform: uppercase; letter-spacing: 0.06em; background: var(--surface-raised); position: sticky; top: 0; z-index: 1; }
  tr.groups th { position: static; height: auto; padding-top: var(--space-2); padding-bottom: var(--space-1); text-align: center; }
  th, td { text-align: right; border-bottom: var(--hairline) solid var(--border); }
  td { height: var(--row-h); padding: var(--space-1) var(--space-3); font-family: var(--mono); }
  th:first-child, td:first-child { text-align: left; }
  td:first-child, td.word { font-family: inherit; }
  td b { font-weight: var(--fw-medium); }
  td small { display: block; color: var(--dim); font-size: var(--fs-metric-label); margin-top: 2px; }
  tbody tr:last-child td { border-bottom: 0; }
  /* Simulated reads quieter than real: the engine's numbers in the muted ink, the venue's in
     the text ink with the accent rule on the left of its block. */
  .sim { color: var(--muted); }
  th.sim { font-style: italic; }
  th.real, td.real { color: var(--text); }
  th.real.split { border-left: var(--active-rule) solid var(--accent); }
  td.real:nth-child(5) { border-left: var(--active-rule) solid color-mix(in srgb, var(--accent) 45%, transparent); }
  /* The source of a column is an icon in its header (flask = engine, briefcase = venue), named
     by the tooltip: no banner row above the books. */
  .src { display: inline-flex; align-items: center; gap: var(--space-1); }
  th.sim .src > :global(svg) { color: var(--muted); opacity: 0.7; }
  th.real .src > :global(svg) { color: var(--accent); }
  .drift { color: var(--green); }
  .drift.off { color: var(--amber); font-weight: var(--fw-medium); }
  .order { white-space: nowrap; }
  .worse { color: var(--red); }
  .better { color: var(--green); }
  .outcome.refused, .outcome.unknown { color: var(--red); border-color: color-mix(in srgb, var(--red) 45%, transparent); }
  .sub-head { display: flex; align-items: baseline; justify-content: space-between; gap: var(--space-3); padding: var(--space-4) var(--space-4) var(--space-2); border-top: var(--hairline) solid var(--border); }
  .sub-head span { color: var(--dim); font-size: var(--fs-desc); }
  .no-trades, .empty-row { color: var(--dim); text-align: center; }
  .no-trades { display: flex; flex-direction: column; gap: var(--space-1); padding: var(--space-6) var(--space-4); font-size: var(--fs-desc); }
  .no-trades b { color: var(--text); font-weight: var(--fw-medium); font-size: var(--text-sm); }
  .empty-row { font-family: inherit; }
  .confirm { display: flex; flex-direction: column; gap: var(--space-3); font-size: var(--text-sm); }
  .confirm p { margin: 0; line-height: var(--lh-base); }
  .live-warn { color: var(--red); display: flex; gap: var(--space-2); align-items: flex-start; }
  @media (max-width: 560px) {
    .section-head { flex-direction: column; }
    .facts { gap: var(--space-2) var(--space-4); }
  }
</style>
