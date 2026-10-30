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
  import { paperApi } from './paper.js';
  import { bookRows, compareRows, execState, nextStep, num, stateTone } from './paper-execution.js';
  import { t } from '$lib/i18n';

  let { sessionId, onchanged = () => {} } = $props();
  const label = (key, params) => $t(`backtest.paper.exec.${key}`, params);
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
      const next = await paperApi.execution(sessionId);
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
      if (act === 'arm') await paperApi.arm(sessionId, code);
      else if (act === 'flatten') await paperApi.flatten(sessionId, code);
      else if (act === 'adopt') await paperApi.adopt(sessionId);
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
      await paperApi.disarm(sessionId);
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
  <section class="card exec" data-paper-execution>
    <div class="section-head">
      <div>
        <h3><Icon name="briefcase" size={14} /> {label('title')}</h3>
        <p>
          {view.account?.name ?? view.execution?.account ?? ''}
          <span class="level {level}">{label(`level.${level}`)}</span>
        </p>
      </div>
      <span class="chip {tone}"><span class="dot"></span>{label(`state.${state}`)}</span>
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
          <tr class="groups">
            <th></th>
            <th colspan="3" class="sim"><Icon name="flask" size={12} /> {label('simulated')}</th>
            <th colspan="4" class="real"><Icon name="briefcase" size={12} /> {label('atBroker')}</th>
          </tr>
          <tr>
            <th>{label('instrument')}</th>
            <th class="sim">{label('position')}</th>
            <th class="sim">{label('stop')}</th>
            <th class="sim">{label('target')}</th>
            <th class="real">{label('held')}</th>
            <th class="real">{label('own')}</th>
            <th class="real">{label('drift')}</th>
            <th class="real">{label('working')}</th>
          </tr>
        </thead>
        <tbody>
          {#each books as b (b.key)}
            <tr>
              <td><b>{b.ticker}</b>{#if b.symbol !== b.ticker}<small>{b.symbol}</small>{/if}</td>
              <td class="sim">
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
      <div class="empty"><b>{label('noTrades')}</b><span>{label('noTradesHint')}</span></div>
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
  .card { border: 1px solid var(--border-control); border-radius: var(--radius-lg); background: var(--surface); overflow: hidden; min-width: 0; }
  .section-head { display: flex; align-items: flex-start; justify-content: space-between; gap: var(--space-3); padding: 16px 18px 8px; }
  .section-head p { margin: 5px 0 0; color: var(--muted); font-size: var(--text-xs); display: flex; align-items: center; gap: var(--space-2); flex-wrap: wrap; }
  h3 { font-size: var(--text-sm); font-weight: 600; margin: 0; display: flex; align-items: center; gap: 7px; }
  h4 { font-size: var(--text-xs); font-weight: 600; margin: 0; }
  .level { padding: 2px 7px; border-radius: var(--radius); background: var(--surface-2); color: var(--muted); }
  .level.live { color: var(--red); background: color-mix(in srgb, var(--red) 8%, transparent); font-weight: 600; }
  .level.paper { color: var(--green); }
  .chip { display: inline-flex; gap: 6px; align-items: center; padding: 4px 9px; border: 1px solid var(--border-control); border-radius: 20px; color: var(--muted); font-size: var(--text-xs); white-space: nowrap; }
  .chip.ok { color: var(--green); border-color: color-mix(in srgb, var(--green) 30%, transparent); }
  .chip.warn { color: var(--amber); border-color: color-mix(in srgb, var(--amber) 30%, transparent); }
  .chip.bad { color: var(--red); border-color: color-mix(in srgb, var(--red) 30%, transparent); }
  .dot { display: inline-block; width: 6px; height: 6px; border-radius: 50%; background: currentColor; }
  .note { display: flex; align-items: flex-start; gap: var(--space-2); margin: var(--space-1) 18px var(--space-2); padding: var(--space-2) var(--space-3); border-radius: var(--radius); font-size: var(--text-xs); line-height: 1.5; background: var(--surface-2); color: var(--muted); }
  .note.warn { color: var(--amber); background: color-mix(in srgb, var(--amber) 7%, transparent); }
  .note.bad { color: var(--red); background: color-mix(in srgb, var(--red) 7%, transparent); }
  .note span { flex: 1; overflow-wrap: anywhere; }
  .facts { display: flex; flex-wrap: wrap; gap: var(--space-2) var(--space-6); padding: var(--space-2) 18px; color: var(--muted); font-size: var(--text-xs); }
  .facts span { display: inline-flex; align-items: center; gap: 6px; }
  .facts b { color: var(--text); font-weight: 500; }
  .limits { cursor: help; }
  .actions { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); padding: var(--space-2) 18px var(--space-3); border-bottom: 1px solid var(--border); }
  .act-error { color: var(--red); font-size: var(--text-xs); margin: 0; }
  .table-scroll { max-height: 320px; overflow: auto; }
  table { border-collapse: collapse; width: 100%; font-size: var(--text-xs); white-space: nowrap; font-variant-numeric: tabular-nums; }
  th { color: var(--muted); font-weight: 500; background: var(--surface-2); position: sticky; top: 0; z-index: 1; }
  tr.groups th { position: static; text-transform: uppercase; letter-spacing: 0.06em; font-size: 10px; padding-top: 8px; padding-bottom: 4px; }
  th, td { text-align: right; padding: 10px 14px; border-bottom: 1px solid var(--border); }
  th:first-child, td:first-child { text-align: left; }
  td b { font-weight: 550; }
  td small { display: block; color: var(--muted); font-size: 10px; margin-top: 3px; }
  tbody tr:last-child td { border-bottom: 0; }
  /* Simulated reads quieter than real: the engine's numbers in the muted ink, the venue's in
     the text ink with the accent rule on the left of its block. */
  .sim { color: var(--muted); }
  th.sim { font-style: italic; }
  th.real, td.real { color: var(--text); }
  tr.groups th.real, tr:not(.groups) th.real:first-of-type { box-shadow: inset 2px 0 0 var(--accent); }
  td.real:nth-child(5) { box-shadow: inset 2px 0 0 color-mix(in srgb, var(--accent) 45%, transparent); }
  .drift { color: var(--green); }
  .drift.off { color: var(--amber); font-weight: 600; }
  .order { white-space: nowrap; }
  .worse { color: var(--red); }
  .better { color: var(--green); }
  .outcome { padding: 2px 7px; border-radius: var(--radius); background: var(--surface-2); color: var(--muted); }
  .outcome.refused, .outcome.unknown { color: var(--red); }
  .sub-head { display: flex; align-items: baseline; justify-content: space-between; gap: var(--space-3); padding: var(--space-4) 18px var(--space-2); border-top: 1px solid var(--border); }
  .sub-head span { color: var(--muted); font-size: var(--text-xs); }
  .empty, .empty-row { color: var(--muted); text-align: center; }
  .empty { display: flex; flex-direction: column; gap: var(--space-1); padding: var(--space-6) var(--space-4); font-size: var(--text-xs); }
  .empty b { color: var(--text); font-weight: 500; font-size: var(--text-sm); }
  .confirm { display: flex; flex-direction: column; gap: var(--space-3); font-size: var(--text-sm); }
  .confirm p { margin: 0; line-height: 1.55; }
  .live-warn { color: var(--red); display: flex; gap: var(--space-2); align-items: flex-start; }
  @media (max-width: 560px) {
    .section-head { flex-direction: column; }
    .facts { gap: var(--space-2) var(--space-4); }
  }
</style>
