<script>
  // A new robot, step by step: the strategy it freezes, the instruments it trades, the account
  // its orders go to, its capital, its guards, when it runs and who hears from it, then a
  // review. It starts from a backtest run (`?run=`), a saved strategy (`?strategy=`) or
  // nothing (pick a saved strategy here). The server checks everything again on create and
  // its refusal is shown as it said it.
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import Button from '$lib/ui/Button.svelte';
  import Input from '$lib/ui/Input.svelte';
  import Select from '$lib/ui/Select.svelte';
  import MultiSelect from '$lib/ui/MultiSelect.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import DataModal from '$lib/modules/histviz/DataModal.svelte';
  import { loadRecents } from '$lib/modules/histviz/pane.js';
  import SchedulePicker from '$lib/modules/automator/SchedulePicker.svelte';
  import ExclusionEditor from './ExclusionEditor.svelte';
  import RobotInitModal from './RobotInitModal.svelte';
  import ChannelButton from '$lib/notifications/ChannelButton.svelte';
  import BackgroundTradingModal from '$lib/brokers/BackgroundTradingModal.svelte';
  import CapitalEditor from './CapitalEditor.svelte';
  import GuardEditor from './GuardEditor.svelte';
  import { backtestApi, migrateLegacyOperands } from '$lib/modules/backtest/api.js';
  import { embedIndicators } from '$lib/modules/backtest/embed.js';
  import { indicatorLib } from '$lib/indicators/api.js';
  import { brokersApi } from '$lib/brokers/api.js';
  import { channelsApi } from '$lib/notifications/api.js';
  import { robotsApi, defaultSession, NOTIFY_WHEN } from './api.js';
  import { capitalBody } from './fleet.js';
  import { t } from '$lib/i18n';

  let { runId = null, strategyId = null } = $props();

  const STEPS = ['strategy', 'instruments', 'account', 'capital', 'guards', 'schedule', 'review'];
  let step = $state('strategy');
  const index = $derived(STEPS.indexOf(step));

  let draft = $state(defaultSession('', [], {}));
  let datasets = $state([]);
  let strategies = $state([]);
  let accounts = $state([]);
  let channels = $state([]);
  // The instruments it trades, as the chart's lookup returned them: coordinates, not stored
  // series. A robot trades forward; the server makes each one an empty dataset its first tick
  // fills. One timeframe for all of them (a basket shares one clock).
  let instruments = $state([]);
  let timeframe = $state('');
  let symbols = $state({});
  let source = $state(''); // 'run' | 'strategy' | ''
  let pickedStrategy = $state('');
  let error = $state('');
  let saving = $state(false);
  let loading = $state(true);
  let signOpen = $state(false);
  let pickOpen = $state(false);
  // The robot just created: its first tick is followed in a modal before its page opens.
  let created = $state(null);
  let initOpen = $state(false);
  const recents = loadRecents();

  const tradable = $derived(accounts.filter((a) => (a.modules ?? []).includes('trading')));
  const target = $derived(draft.execution?.target === 'broker' ? 'broker' : 'simulate');
  const account = $derived(tradable.find((a) => a.id === draft.execution?.account_id) ?? null);
  const nativeOk = $derived(!!account?.order_caps?.brackets && !!account?.order_caps?.amend);
  const strategyRow = $derived(strategies.find((s) => s.id === (draft.strategy_id ?? pickedStrategy)) ?? null);
  const tickers = $derived(instruments.map((i) => i.ticker));
  const keyOf = (i) => `${i.provider}|${i.asset_type}|${i.ticker}`;
  // The timeframes every picked instrument's provider serves.
  const tfChoices = $derived(
    instruments.length ? instruments.map((i) => i.timeframes ?? []).reduce((a, b) => a.filter((x) => b.includes(x))) : []
  );
  const exitsMode = $derived(draft.execution?.exits || (nativeOk ? 'native' : 'watch'));
  // Trading at the broker needs every instrument resolved at the account; simulated needs nothing.
  const mapped = $derived(instruments.length > 0 && instruments.every((i) => !!symbols[keyOf(i)]?.symbol));

  function addInstrument(c) {
    if (!c?.ticker || instruments.length >= 8 || instruments.some((i) => keyOf(i) === keyOf(c))) return;
    instruments = [...instruments, c];
  }
  function removeInstrument(i) {
    instruments = instruments.filter((x) => keyOf(x) !== keyOf(i));
  }
  // Keep the timeframe one every instrument serves: the first pick's own, else daily, else the first.
  $effect(() => {
    const tfs = tfChoices;
    if (!tfs.length || tfs.includes(timeframe)) return;
    timeframe = instruments[0]?.timeframe && tfs.includes(instruments[0].timeframe) ? instruments[0].timeframe : tfs.includes('1d') ? '1d' : tfs[0];
  });

  async function loadSource() {
    const [ds, strats, inds] = await Promise.all([
      backtestApi.datasets().catch(() => []),
      backtestApi.strategies().catch(() => []),
      indicatorLib.list().catch(() => [])
    ]);
    datasets = ds ?? [];
    strategies = strats ?? [];
    const list = await robotsApi.list().catch(() => null);
    draft.guards = structuredClone(list?.guard_defaults ?? []);
    if (runId) {
      const r = await fetch(`/api/backtest/runs/${runId}`).then((x) => (x.ok ? x.json() : null)).catch(() => null);
      if (r?.run) {
        draft.settings = embedIndicators(migrateLegacyOperands(r.run.settings ?? {}), inds ?? []);
        const ids = r.run.dataset_ids ?? (r.run.dataset_id ? [r.run.dataset_id] : []);
        const rows = ids.map((id) => datasets.find((d) => d.id === id)).filter(Boolean);
        instruments = rows.map((d) => ({ provider: d.provider, asset_type: d.asset_type, ticker: d.ticker, timeframe: d.timeframe, timeframes: [d.timeframe] }));
        timeframe = rows[0]?.timeframe ?? '';
        draft.strategy_id = strategyId ?? r.run.strategy_id ?? null;
        source = 'run';
      }
    }
    if (!source && strategyId) await useStrategy(strategyId);
    const s = strategies.find((x) => x.id === draft.strategy_id);
    draft.name = s?.name ?? (source === 'run' ? $t('robots.wizard.fromRun') : '');
  }

  async function useStrategy(id) {
    pickedStrategy = id;
    const full = await backtestApi.getStrategy(id).catch(() => null);
    if (!full) return;
    draft.settings = migrateLegacyOperands(full.settings ?? {});
    draft.strategy_id = id;
    if (!draft.name) draft.name = full.name;
    source = 'strategy';
  }

  onMount(async () => {
    try {
      await loadSource();
      accounts = (await brokersApi.list('robots').catch(() => [])) ?? [];
      channels = (await channelsApi.list('robots').catch(() => [])) ?? [];
    } finally {
      loading = false;
    }
  });

  // Each instrument as the account spells it, asked of its own lookup (never by name).
  $effect(() => {
    const id = account?.id;
    const rows = instruments.slice();
    if (!id) {
      symbols = {};
      return;
    }
    symbols = Object.fromEntries(rows.map((i) => [keyOf(i), { loading: true }]));
    for (const i of rows) {
      const k = keyOf(i);
      brokersApi
        .symbolFor(id, i.ticker, i.asset_type)
        .then((r) => (symbols = { ...symbols, [k]: { symbol: r.symbol, step: r.step } }))
        .catch((e) => (symbols = { ...symbols, [k]: { error: e.message } }));
    }
  });

  function setTarget(next) {
    draft.execution = next === 'broker' ? { target: 'broker', account_id: tradable[0]?.id ?? null, exits: '', max_position_qty: '' } : null;
  }

  function body() {
    const e = draft.execution;
    const max = String(e?.max_position_qty ?? '').trim();
    return {
      ...draft,
      dataset_ids: [],
      // The data source picked with each instrument: the robot reads its bars through it.
      instruments: instruments.map((i) => ({ provider: i.provider, asset_type: i.asset_type, ticker: i.ticker, timeframe, connector_id: i.connector_id ?? null })),
      capital: capitalBody(draft.capital),
      priority: Number(draft.priority) || 0,
      execution: e?.target === 'broker' ? { target: 'broker', account_id: e.account_id, exits: e.exits || (nativeOk ? 'native' : 'watch'), max_position_qty: max || null } : null
    };
  }

  const canNext = $derived(
    step === 'strategy' ? !!draft.name.trim() && Object.keys(draft.settings ?? {}).length > 0 :
    step === 'instruments' ? instruments.length > 0 && !!timeframe :
    step === 'account' ? target === 'simulate' || (!!account && mapped) :
    true
  );

  async function create() {
    error = '';
    saving = true;
    try {
      created = await robotsApi.create(body());
      initOpen = true;
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }

  const notifyOptions = $derived(NOTIFY_WHEN.map((v) => ({ value: v, label: $t(`robots.notify.${v}`) })));
  const channelOptions = $derived(channels.map((c) => ({ value: c.id, label: `${c.name} (${c.kind})` })));
  const strategyOptions = $derived(strategies.map((s) => ({ value: s.id, label: s.name })));
</script>

<div class="wizard" data-robot-wizard>
  <ol class="steps" aria-label={$t('robots.wizard.title')}>
    {#each STEPS as s, i (s)}
      <li class:on={s === step} class:done={i < index}>
        <button type="button" disabled={i > index} onclick={() => (step = s)}>
          <span class="n">{i + 1}</span>{$t(`robots.wizard.step.${s}`)}
        </button>
      </li>
    {/each}
  </ol>

  {#if loading}
    <p class="hint">{$t('robots.dashboard.loading')}</p>
  {:else}
    <section class="body">
      {#if step === 'strategy'}
        <h3>{$t('robots.wizard.step.strategy')}</h3>
        <p class="hint">{$t('robots.wizard.strategyHint')}</p>
        <Input label={$t('robots.name')} bind:value={draft.name} required />
        {#if source === 'run'}
          <p class="frozen"><Icon name="lock" size={12} /> {$t('robots.wizard.frozenRun')}{#if strategyRow} · {strategyRow.name}{/if}</p>
        {:else}
          <div class="pick">
            <Select label={$t('robots.wizard.savedStrategy')} options={strategyOptions} value={pickedStrategy} onpick={useStrategy} placeholder={$t('robots.wizard.pickStrategy')} />
            <Button variant="ghost" icon="plus" onclick={() => goto('/backtest')}>{$t('robots.wizard.newStrategy')}</Button>
          </div>
          {#if !strategies.length}<p class="warn">{$t('robots.wizard.noStrategies')}</p>{/if}
        {/if}
      {:else if step === 'instruments'}
        <h3>{$t('robots.wizard.step.instruments')}</h3>
        <p class="hint">{$t('robots.wizard.instrumentsHint')}</p>
        {#each instruments as i (keyOf(i))}
          <p class="map">
            <Badge>{i.ticker}</Badge><span class="hint">{i.provider} · {i.asset_type}</span>
            <button class="link" aria-label={$t('common.remove')} onclick={() => removeInstrument(i)}><Icon name="x" size={12} /></button>
          </p>
          {#if i.stream !== undefined}
            <p class="hint" class:warn={!i.stream}>{i.stream ? $t('robots.liveYes', { ticker: i.ticker, provider: i.provider }) : $t('robots.liveNo', { ticker: i.ticker, provider: i.provider })}</p>
          {/if}
        {/each}
        <div class="pick">
          <Button variant="ghost" icon="search" disabled={instruments.length >= 8} onclick={() => (pickOpen = true)}>{$t('robots.wizard.addInstrument')}</Button>
          {#if instruments.length}
            <Select label={$t('robots.wizard.timeframe')} options={tfChoices.map((v) => ({ value: v, label: v }))} bind:value={timeframe} />
          {/if}
        </div>
        {#if instruments.length && !tfChoices.length}<p class="warn">{$t('robots.wizard.noSharedTimeframe')}</p>{/if}
      {:else if step === 'account'}
        <h3>{$t('robots.wizard.step.account')}</h3>
        <div class="seg" role="group" aria-label={$t('robots.exec.section')}>
          <button class:on={target === 'simulate'} onclick={() => setTarget('simulate')}>{$t('robots.exec.simulate')}</button>
          <button class:on={target === 'broker'} disabled={!tradable.length} onclick={() => setTarget('broker')}>{$t('robots.exec.broker')}</button>
        </div>
        <p class="hint">{target === 'simulate' ? $t('robots.exec.simulateHint') : $t('robots.exec.brokerHint')}</p>
        {#if target === 'simulate' && !tradable.length}<p class="hint">{$t('robots.exec.noAccount')}</p>{/if}
        {#if target === 'broker'}
          {#if !tradable.length}
            <p class="warn">{$t('robots.exec.noAccount')}</p>
          {:else}
            <div class="grid">
              <Select label={$t('robots.exec.account')} options={tradable.map((a) => ({ value: a.id, label: `${a.name} (${a.capability?.label ?? a.broker})` }))} bind:value={draft.execution.account_id} />
              <Input label={$t('robots.exec.maxPosition')} bind:value={draft.execution.max_position_qty} inputmode="decimal" hint={$t('robots.exec.maxHint')} />
            </div>
            {#if account}
              {#each instruments as i (keyOf(i))}
                {@const m = symbols[keyOf(i)]}
                <p class="map"><Badge>{i.ticker}</Badge><Icon name="arrow-right" size={12} />
                  {#if m?.loading}<span class="hint">{$t('robots.exec.lookup')}</span>
                  {:else if m?.symbol}<Badge tone="accent">{m.symbol}</Badge>{#if m.step}<span class="hint">{$t('robots.exec.step', { step: m.step })}</span>{/if}
                  {:else}<span class="warn">{m?.error ?? $t('robots.exec.unmapped', { ticker: i.ticker })}</span>{/if}
                </p>
              {/each}
              <div class="fld">
                <span class="lbl">{$t('robots.exec.exits')}</span>
                <div class="seg" role="group" aria-label={$t('robots.exec.exits')}>
                  <button class:on={exitsMode === 'native'} disabled={!nativeOk} onclick={() => (draft.execution.exits = 'native')}>{$t('robots.exec.exitsNative')}</button>
                  <button class:on={exitsMode === 'watch'} onclick={() => (draft.execution.exits = 'watch')}>{$t('robots.exec.exitsWatch')}</button>
                </div>
                <p class="hint">{$t(`robots.exec.exits.${exitsMode}`)}</p>
                {#if !nativeOk}<p class="hint">{$t('robots.exec.nativeUnavailable')}</p>{/if}
              </div>
              {#if account.background_trading_ack}
                <p class="hint"><Icon name="check" size={12} /> {$t('robots.exec.backgroundOn')}</p>
              {:else}
                <p class="warn">{$t('robots.exec.backgroundOff')} <button class="link" onclick={() => (signOpen = true)}>{$t('robots.exec.signNow')}</button></p>
              {/if}
              {#if account.live_since}<p class="warn">{$t('robots.exec.armBodyLive')}</p>{/if}
            {/if}
          {/if}
        {/if}
      {:else if step === 'capital'}
        <h3>{$t('robots.wizard.step.capital')}</h3>
        <p class="hint">{$t('robots.capital.hint')}</p>
        <CapitalEditor bind:capital={draft.capital} currency={account?.config?.currency ?? ''} broker={draft.execution?.target === 'broker'} />
      {:else if step === 'guards'}
        <h3>{$t('robots.wizard.step.guards')}</h3>
        <p class="hint">{$t('robots.guard.hint')}</p>
        <GuardEditor bind:guards={draft.guards} />
      {:else if step === 'schedule'}
        <h3>{$t('robots.wizard.step.schedule')}</h3>
        <p class="hint">{$t('robots.tickHint', { timeframe })}</p>
        <h4>{$t('robots.exclusions.title')}</h4>
        <p class="hint">{$t('robots.exclusions.hint', { timezone: draft.timezone })}</p>
        <ExclusionEditor bind:exclusions={draft.exclusions} />
        <h4>{$t('robots.report.title')}</h4>
        <label class="check">
          <input type="checkbox" checked={draft.cadence === 'scheduled'} onchange={(e) => (draft.cadence = e.currentTarget.checked ? 'scheduled' : 'each')} />
          <span>{$t('robots.report.on')}<em>{$t('robots.report.hint')}</em></span>
        </label>
        {#if draft.cadence === 'scheduled'}
          <SchedulePicker bind:rule={draft} catchUp={false} />
        {/if}
        <div class="grid">
          <Input label={$t('robots.windowBars')} type="number" bind:value={draft.window_bars} hint={$t('robots.windowHint')} />
          <Input label={$t('robots.priority')} type="number" bind:value={draft.priority} hint={$t('robots.priorityHint')} />
          <Select label={$t('robots.notifyWhen')} options={notifyOptions} bind:value={draft.notify_when} />
          <div class="fld">
            <span class="lbl">{$t('robots.channels')}</span>
            <MultiSelect bind:value={draft.channel_ids} options={channelOptions} allLabel={$t('robots.allChannels')} width="100%" />
          </div>
        </div>
        <ChannelButton module="robots" size="sm" onchanged={async () => (channels = (await channelsApi.list('robots').catch(() => [])) ?? [])} />
      {:else}
        <h3>{$t('robots.wizard.step.review')}</h3>
        <dl class="review">
          <dt>{$t('robots.name')}</dt><dd>{draft.name}</dd>
          <dt>{$t('robots.wizard.step.strategy')}</dt><dd>{strategyRow?.name ?? $t('robots.wizard.fromRun')}</dd>
          <dt>{$t('robots.wizard.step.instruments')}</dt><dd>{tickers.join(', ')} · {timeframe}</dd>
          <dt>{$t('robots.wizard.step.account')}</dt><dd>{target === 'broker' ? $t('robots.exec.summaryBroker', { account: account?.name ?? '' }) : $t('robots.exec.summarySimulate')}</dd>
          <dt>{$t('robots.wizard.step.capital')}</dt><dd class="fig">{Object.keys(capitalBody(draft.capital)).length ? Object.entries(capitalBody(draft.capital)).map(([k, v]) => `${$t(`robots.capital.short.${k}`)} ${v}`).join(' · ') : $t('robots.capital.noCap')}</dd>
          <dt>{$t('robots.wizard.step.guards')}</dt><dd class="fig">{$t('robots.wizard.guardCount', { n: draft.guards.length })}</dd>
          <dt>{$t('robots.wizard.step.schedule')}</dt><dd>{$t('robots.tickSummary', { timeframe })}{draft.cadence === 'scheduled' ? ` · ${$t('robots.report.title')} ${$t(`automator.sched.kind.${draft.kind}`)}` : ''} · {draft.timezone}</dd>
        </dl>
        {#if target === 'broker'}<p class="hint">{$t('robots.wizard.armLater')}</p>{/if}
      {/if}

      {#if error}<p class="err" role="alert">{error}</p>{/if}
    </section>

    <div class="nav">
      <Button variant="ghost" disabled={index === 0} onclick={() => (step = STEPS[index - 1])}>{$t('robots.wizard.back')}</Button>
      {#if step === 'review'}
        <Button variant="primary" icon="bot" loading={saving} onclick={create}>{$t('robots.wizard.create')}</Button>
      {:else}
        <Button variant="primary" disabled={!canNext} onclick={() => (step = STEPS[index + 1])}>{$t('robots.wizard.next')}</Button>
      {/if}
    </div>
  {/if}
</div>

<DataModal bind:open={pickOpen} {datasets} {recents} onselect={addInstrument} />

<RobotInitModal bind:open={initOpen} robot={created} onclose={() => goto(`/robots/${created.id}`)} />
<BackgroundTradingModal bind:open={signOpen} {account} onsigned={async () => { signOpen = false; accounts = (await brokersApi.list('robots').catch(() => [])) ?? []; }} />

<style>
  .wizard {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    max-width: 860px;
  }
  /* Stepper on the house tab pattern: mono step number, the current step underlined with
     the accent rule, done steps in ink, future steps dim. */
  .steps {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    list-style: none;
    margin: 0;
    padding: 0;
    border-bottom: var(--hairline) solid var(--border);
  }
  .steps button {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    border: 0;
    border-bottom: var(--active-rule) solid transparent;
    margin-bottom: -1px;
    background: transparent;
    color: var(--dim);
    padding: var(--space-2) var(--space-3);
    font: inherit;
    font-size: var(--fs-body);
    cursor: pointer;
    white-space: nowrap;
  }
  .steps button:hover:not(:disabled) {
    color: var(--text);
  }
  .steps button:disabled {
    cursor: default;
  }
  .steps li.done button {
    color: var(--text);
  }
  .steps li.on button {
    color: var(--text);
    font-weight: var(--fw-medium);
    border-bottom-color: var(--accent);
  }
  .n {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: var(--fs-metric-label);
    color: var(--dim);
  }
  /* Level-1 panel: square, one hairline, no lift. */
  .body {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    border: var(--hairline) solid var(--border);
    background: var(--surface);
    padding: var(--space-4);
  }
  h3 {
    margin: 0;
    font-size: var(--fs-item-title);
    font-weight: var(--fw-medium);
    letter-spacing: 0.02em;
  }
  .hint {
    margin: 0;
    font-size: var(--fs-desc);
    color: var(--dim);
  }
  h4 {
    margin: var(--space-2) 0 0;
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
  }
  .check {
    display: flex;
    gap: var(--space-2);
    align-items: flex-start;
    font-size: var(--text-sm);
  }
  .check em {
    display: block;
    font-style: normal;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .warn {
    margin: 0;
    font-size: var(--fs-desc);
    color: var(--amber);
  }
  .err {
    margin: 0;
    color: var(--red);
    font-size: var(--text-sm);
    white-space: pre-wrap;
  }
  .frozen {
    margin: 0;
    display: flex;
    align-items: center;
    gap: var(--space-1);
    font-size: var(--fs-desc);
    color: var(--dim);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: var(--space-3);
    align-items: start;
  }
  /* A picker with its side action: the picker takes the room, the button sits on its baseline. */
  .pick {
    display: flex;
    align-items: flex-end;
    gap: var(--space-2);
  }
  .pick > :global(:first-child) {
    flex: 1;
    min-width: 0;
  }
  .fld {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }
  .lbl {
    color: var(--dim);
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
  }
  /* Segmented choice: one frame, borderless segments, the pressed one filled. */
  .seg {
    display: inline-flex;
    border: var(--hairline) solid var(--border-control);
    overflow: hidden;
    align-self: flex-start;
  }
  .seg button {
    border: 0;
    background: transparent;
    color: var(--muted);
    padding: var(--space-1) var(--space-3);
    font: inherit;
    font-size: var(--fs-body);
    cursor: pointer;
  }
  .seg button:hover:not(:disabled) {
    color: var(--text);
  }
  .seg button.on {
    background: var(--surface-2);
    color: var(--text);
  }
  .seg button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .map {
    margin: 0;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
    color: var(--dim);
  }
  /* Ticker and venue symbol: square hairline tags in mono, neutral ink. */
  .map :global(.badge) {
    border-radius: 0;
    background: transparent;
    padding: 0 var(--space-1);
    font-family: var(--mono);
    text-transform: none;
    letter-spacing: 0;
  }
  .map :global(.badge.accent) {
    color: var(--text);
    border-color: var(--border-control);
  }
  .link {
    border: 0;
    background: none;
    color: var(--text);
    text-decoration: underline;
    text-underline-offset: 2px;
    cursor: pointer;
    font: inherit;
    padding: 0;
  }
  .review {
    display: grid;
    grid-template-columns: max-content minmax(0, 1fr);
    align-items: baseline;
    gap: var(--space-3) var(--space-6);
    margin: 0;
    font-size: var(--fs-body);
  }
  .review dt {
    color: var(--dim);
    font-size: var(--fs-metric-label);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .review dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .review dd.fig {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  .nav {
    display: flex;
    justify-content: space-between;
    gap: var(--space-3);
  }
  @media (max-width: 560px) {
    .grid {
      grid-template-columns: minmax(0, 1fr);
    }
    .steps {
      flex-wrap: nowrap;
      overflow-x: auto;
    }
  }
</style>
