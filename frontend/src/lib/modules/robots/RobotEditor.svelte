<script>
  // A robot's settings: the whole robot on one screen, from the strategy it froze to the
  // wording of the message it will send, with its capital policy, its guards and its priority.
  //
  // The strategy and the instruments are shown, not edited: a session is started from a
  // result the user is looking at, and editing them here would silently change what is being
  // traded forward. Everything else (when, how loud, what the message says) is the point of
  // this form.
  import Modal from '$lib/ui/Modal.svelte';
  import Input from '$lib/ui/Input.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Select from '$lib/ui/Select.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import MultiSelect from '$lib/ui/MultiSelect.svelte';
  import SchedulePicker from '$lib/modules/automator/SchedulePicker.svelte';
  import ExclusionEditor from './ExclusionEditor.svelte';
  import ChannelButton from '$lib/notifications/ChannelButton.svelte';
  import { channelsApi } from '$lib/notifications/api.js';
  import { connectorsApi } from '$lib/connectors/api.js';
  import { robotsApi, NOTIFY_WHEN, FEEDS, MESSAGE_KINDS } from './api.js';
  import { backtestApi } from '$lib/modules/backtest/api.js';
  import { brokersApi } from '$lib/brokers/api.js';
  import BackgroundTradingModal from '$lib/brokers/BackgroundTradingModal.svelte';
  import CapitalEditor from './CapitalEditor.svelte';
  import GuardEditor from './GuardEditor.svelte';
  import { capitalBody } from './fleet.js';
  import { t } from '$lib/i18n';

  let {
    open = $bindable(false),
    session = $bindable(null),
    tickers = [],
    onsaved = () => {}
  } = $props();

  // The instruments the session actually trades: the filter offers those, never free text.
  const tickerOptions = $derived(tickers.map((tk) => ({ value: tk, label: tk })));
  // The recap message only exists when the session sends one: fills go out on every tick.
  const grouping = $derived(session?.cadence !== 'each');

  let saving = $state(false);
  let error = $state('');
  let channels = $state([]);
  let vars = $state([]);
  let filters = $state([]);
  // Which of the three messages is being written. The preview and the helper list follow
  // it, so what is shown is always the vocabulary of the message on screen.
  let msgKind = $state('exit');
  let preview = $state({ title: '', message: '' });
  let helperOpen = $state(false);
  /** Which template the helper list inserts into, and the DOM node holding the caret. The
   *  node comes from the focus event: the shared Input renders it, so there is no ref to bind. */
  let focused = $state('body');
  let focusedEl = $state(null);

  const editing = $derived(!!session?.id);

  // Whether each instrument's provider streams a live price: stops, targets and limits then
  // trigger on it, otherwise at candle close, and the user is told before starting.
  let liveData = $state([]);
  // The data sources a robot may read through, for the per-instrument picker.
  let dataSources = $state([]);
  $effect(() => {
    if (!open) return;
    connectorsApi.list('robots').then((c) => (dataSources = c ?? [])).catch(() => (dataSources = []));
  });
  function sourceOptions(provider) {
    return [
      { value: '', label: $t('robots.dataSource.default') },
      ...dataSources.filter((c) => c.provider === provider).map((c) => ({ value: c.id, label: c.name }))
    ];
  }
  // The strategy's order types: its own, shown here, set where the strategy is built.
  const orderLabel = (o) => $t(o?.kind === 'limit' ? 'backtest.exec.limit' : 'backtest.exec.market');
  function setSource(datasetId, id) {
    const next = { ...(session.data_connectors ?? {}) };
    if (id) next[datasetId] = id;
    else delete next[datasetId];
    session.data_connectors = next;
  }
  $effect(() => {
    const ids = (session?.dataset_ids ?? []).slice();
    if (!open || !ids.length) {
      liveData = [];
      return;
    }
    backtestApi
      .executionData(ids)
      .then((d) => (liveData = d ?? []))
      .catch(() => (liveData = []));
  });
  // Where the fills happen. Simulate is the session as it always was; a broker account makes
  // the engine's decisions orders on that account, once the session is armed from its
  // dashboard. Chosen at creation only: an edit shows it, never changes it.
  let accounts = $state([]);
  let symbols = $state({});
  let signOpen = $state(false);
  const target = $derived(session?.execution?.target === 'broker' ? 'broker' : 'simulate');
  const tradable = $derived(accounts.filter((a) => (a.modules ?? []).includes('trading')));
  const account = $derived(tradable.find((a) => a.id === session?.execution?.account_id) ?? null);
  const nativeOk = $derived(!!account?.order_caps?.brackets && !!account?.order_caps?.amend);
  const execLabel = $derived(
    target === 'broker'
      ? $t('robots.exec.summaryBroker', { account: account?.name ?? session?.execution?.account ?? '' })
      : $t('robots.exec.summarySimulate')
  );
  async function loadAccounts() {
    try {
      accounts = (await brokersApi.list('robots')) ?? [];
    } catch {
      accounts = [];
    }
  }
  $effect(() => {
    if (open && !editing) loadAccounts();
  });
  function setTarget(next) {
    if (next === 'broker') {
      session.execution = { target: 'broker', account_id: tradable[0]?.id ?? null, exits: '', max_position_qty: '' };
    } else {
      session.execution = null;
    }
  }
  // Each instrument as the account spells it, asked of the account's own lookup: what the
  // server will check again at save time, shown before it.
  $effect(() => {
    const id = account?.id;
    const rows = liveData.slice();
    if (!open || editing || !id) {
      symbols = {};
      return;
    }
    symbols = Object.fromEntries(rows.map((d) => [d.dataset_id, { loading: true }]));
    for (const d of rows) {
      brokersApi
        .symbolFor(id, d.ticker, d.asset_type)
        .then((r) => (symbols = { ...symbols, [d.dataset_id]: { symbol: r.symbol, step: r.step } }))
        .catch((e) => (symbols = { ...symbols, [d.dataset_id]: { error: e.message } }));
    }
  });
  // The exits a broker can hold: attached and moved there, or watched on the live price.
  $effect(() => {
    if (target === 'broker' && session.execution && session.execution.exits === 'native' && account && !nativeOk) {
      session.execution.exits = 'watch';
    }
  });
  /** The execution as the server takes it: decimals as strings, nothing empty. */
  function executionBody() {
    const e = session.execution;
    if (!e || e.target !== 'broker') return null;
    const max = String(e.max_position_qty ?? '').trim();
    return { target: 'broker', account_id: e.account_id, exits: e.exits || (nativeOk ? 'native' : 'watch'), max_position_qty: max || null };
  }

  // The summary only exists when a cadence asks for one.
  const kinds = $derived(MESSAGE_KINDS.filter((k) => k.id !== 'summary' || grouping));
  const current = $derived(kinds.find((k) => k.id === msgKind) ?? MESSAGE_KINDS[1]);
  $effect(() => {
    if (!kinds.some((k) => k.id === msgKind)) msgKind = 'exit';
  });

  // Each section collapses to its own answer: the header carries the value, so the form
  // reads as a list of decisions before it is opened as a list of controls.
  const feedLabel = $derived($t(`robots.feed.${session?.feed ?? 'hist'}`));
  const reportLabel = $derived(
    session?.cadence === 'scheduled'
      ? `${$t(`automator.sched.kind.${session.kind}`)} · ${session.timezone ?? ''}`.trim()
      : $t('robots.report.off')
  );
  const ex = $derived(session?.exclusions ?? {});
  const exclusionLabel = $derived(
    (ex.weekdays || ex.hours?.length || ex.dates?.length)
      ? $t('robots.exclusions.set', { timezone: session?.timezone ?? '' })
      : $t('robots.exclusions.none')
  );
  const windowLabel = $derived(
    $t('robots.windowSummary', { count: session?.window_bars ?? 0 })
  );
  const reportingLabel = $derived.by(() => {
    if (!session) return '';
    const when = $t(`robots.notify.${session.notify_when}`);
    const n = session.channel_ids?.length ?? 0;
    const where =
      n === 0
        ? $t('robots.allChannels')
        : $t('robots.channelCount', { count: n });
    return session.notify_when === 'never' ? when : `${when} · ${where}`;
  });
  const messageLabel = $derived(
    session && MESSAGE_KINDS.some((k) => session[k.title] || session[k.body])
      ? $t('robots.customWording')
      : $t('robots.defaultWording')
  );

  const notifyOptions = $derived(
    NOTIFY_WHEN.map((v) => ({ value: v, label: $t(`robots.notify.${v}`) }))
  );
  const channelOptions = $derived(
    channels.map((c) => ({ value: c.id, label: `${c.name} (${c.kind})` }))
  );

  $effect(() => {
    if (!open) return;
    loadChannels();
  });

  // The vocabulary is per message kind, so it is re-read when the tab changes.
  $effect(() => {
    if (!open) return;
    loadVars(current.event);
  });

  async function loadChannels() {
    try {
      channels = await channelsApi.list('robots');
    } catch {
      channels = [];
    }
  }

  async function loadVars(event) {
    try {
      const r = await robotsApi.variables(session?.id ?? null, event);
      vars = r.variables ?? [];
      filters = r.filters ?? [];
    } catch {
      vars = [];
    }
  }

  // The preview is the server's renderer, not a local imitation: what it shows is the
  // message that would be sent, including the error a bad path produces.
  let previewTimer;
  $effect(() => {
    if (!open || !session) return;
    const body = {
      name: session.name,
      entry_title_template: session.entry_title_template,
      entry_template: session.entry_template,
      exit_title_template: session.exit_title_template,
      exit_template: session.exit_template,
      summary_title_template: session.summary_title_template,
      summary_template: session.summary_template,
      event: current.event
    };
    clearTimeout(previewTimer);
    previewTimer = setTimeout(async () => {
      try {
        preview = await robotsApi.previewDraft(body);
      } catch (e) {
        preview = { title: '', message: e.message };
      }
    }, 250);
  });

  /** Insert a path at the caret of whichever template of the open tab was last focused. */
  function insert(path) {
    const el = focusedEl;
    const token = `{{${path}}}`;
    const field = focused === 'title' ? current.title : current.body;
    const text = session[field] ?? '';
    const at = el?.selectionStart ?? text.length;
    session[field] = text.slice(0, at) + token + text.slice(el?.selectionEnd ?? at);
    queueMicrotask(() => {
      el?.focus();
      const pos = at + token.length;
      el?.setSelectionRange?.(pos, pos);
    });
  }

  async function save() {
    error = '';
    saving = true;
    try {
      const body = { ...session, capital: capitalBody(session.capital), priority: Number(session.priority) || 0 };
      const saved = session.id
        ? await robotsApi.update(session.id, body)
        : await robotsApi.create({ ...body, execution: executionBody() });
      open = false;
      onsaved(saved);
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }
</script>

<Modal
  bind:open
  size="lg"
  title={editing ? $t('robots.edit') : $t('robots.start')}
>
  {#if session}
    <div class="form">
      <!-- Identity + what is traded. Frozen at creation: the session must not follow
           later edits of the strategy it came from. -->
      <section class="sec first">
        <div class="grid">
          <div class="span2">
            <Input label={$t('robots.name')} bind:value={session.name} required />
          </div>
        </div>
        <div class="frozen">
          <span class="fz-label"><Icon name="lock" size={11} />{$t('robots.frozen')}</span>
          <span class="chips">
            {#each tickers as ticker (ticker)}<span class="chip">{ticker}</span>{/each}
          </span>
          <span class="fz-orders">
            {$t('backtest.exec.entryOrder')}: {orderLabel(session.settings?.execution?.entry_order)} · {$t('backtest.exec.exitOrder')}: {orderLabel(session.settings?.execution?.exit_order)}
          </span>
          <span class="fz-hint">{$t('robots.frozenHint')}</span>
        </div>
        {#each liveData as d (d.dataset_id)}
          <Select
            label={$t('robots.dataSource.label', { ticker: d.ticker })}
            options={sourceOptions(d.provider)}
            value={session.data_connectors?.[d.dataset_id] ?? ''}
            onpick={(v) => setSource(d.dataset_id, v)}
          />
          {#if d.live}
            <p class="live-note">{$t('robots.liveYes', { ticker: d.ticker, provider: d.provider })}</p>
          {:else}
            <p class="live-note warn">{$t('robots.liveNo', { ticker: d.ticker, provider: d.provider })}</p>
          {/if}
        {/each}
      </section>

      <details class="sec" open>
        <summary class="sec-head">
          <span class="sec-title">
            <Icon name="chevron-right" size={12} />
            <h4>{$t('robots.feed')}</h4>
          </span>
          <span class="sec-value">{feedLabel}</span>
        </summary>
        <p class="sec-desc">{$t('robots.feedHint')}</p>
        <div class="seg" role="group" aria-label={$t('robots.feed')}>
          {#each FEEDS as f (f)}
            <button
              class:on={(session.feed ?? 'hist') === f}
              disabled={f === 'live'}
              title={f === 'live' ? $t('robots.feedSoon') : ''}
              onclick={() => (session.feed = f)}
            >
              {$t(`robots.feed.${f}`)}
            </button>
          {/each}
        </div>
      </details>

      <details class="sec" open={!editing}>
        <summary class="sec-head">
          <span class="sec-title">
            <Icon name="chevron-right" size={12} />
            <h4>{$t('robots.exec.section')}</h4>
          </span>
          <span class="sec-value" class:broker-value={target === 'broker'}>{execLabel}</span>
        </summary>
        {#if editing}
          <p class="sec-desc">{$t('robots.exec.frozen')}</p>
        {:else}
          <div class="seg" role="group" aria-label={$t('robots.exec.section')}>
            <button class:on={target === 'simulate'} onclick={() => setTarget('simulate')}>{$t('robots.exec.simulate')}</button>
            <button class:on={target === 'broker'} onclick={() => setTarget('broker')}>{$t('robots.exec.broker')}</button>
          </div>
          {#if target === 'simulate'}
            <p class="sec-desc">{$t('robots.exec.simulateHint')}</p>
          {:else}
            <p class="sec-desc">{$t('robots.exec.brokerHint')}</p>
            {#if !tradable.length}
              <p class="live-note warn">{$t('robots.exec.noAccount')}</p>
            {:else}
              <div class="grid">
                <Select
                  label={$t('robots.exec.account')}
                  options={tradable.map((a) => ({ value: a.id, label: `${a.name} (${a.capability?.label ?? a.broker})` }))}
                  bind:value={session.execution.account_id}
                />
                <Input
                  label={$t('robots.exec.maxPosition')}
                  bind:value={session.execution.max_position_qty}
                  inputmode="decimal"
                  hint={$t('robots.exec.maxHint')}
                />
              </div>
              {#if account}
                <div class="exec-rows">
                  {#each liveData as d (d.dataset_id)}
                    {@const m = symbols[d.dataset_id]}
                    <div class="exec-row">
                      <span class="chip">{d.ticker}</span>
                      <Icon name="arrow-right" size={12} />
                      {#if m?.loading}
                        <span class="hint">{$t('robots.exec.lookup')}</span>
                      {:else if m?.symbol}
                        <span class="chip strong">{m.symbol}</span>
                        {#if m.step}<span class="hint">{$t('robots.exec.step', { step: m.step })}</span>{/if}
                      {:else}
                        <span class="live-note warn">{m?.error ?? $t('robots.exec.unmapped', { ticker: d.ticker })}</span>
                      {/if}
                    </div>
                  {/each}
                </div>
                <div class="fld">
                  <span class="lbl">{$t('robots.exec.exits')}</span>
                  <div class="seg" role="group" aria-label={$t('robots.exec.exits')}>
                    <button class:on={(session.execution.exits || (nativeOk ? 'native' : 'watch')) === 'native'} disabled={!nativeOk} title={nativeOk ? '' : $t('robots.exec.nativeUnavailable')} onclick={() => (session.execution.exits = 'native')}>{$t('robots.exec.exitsNative')}</button>
                    <button class:on={(session.execution.exits || (nativeOk ? 'native' : 'watch')) === 'watch'} onclick={() => (session.execution.exits = 'watch')}>{$t('robots.exec.exitsWatch')}</button>
                  </div>
                  <span class="hint">{$t(`robots.exec.exits.${(session.execution.exits || (nativeOk ? 'native' : 'watch'))}`)}</span>
                </div>
                <div class="exec-bg">
                  {#if account.background_trading_ack}
                    <span class="hint"><Icon name="check" size={12} /> {$t('robots.exec.backgroundOn')}</span>
                  {:else}
                    <span class="live-note warn">{$t('robots.exec.backgroundOff')}</span>
                    <button class="fz-edit" onclick={() => (signOpen = true)}>{$t('robots.exec.signNow')}</button>
                  {/if}
                  {#if account.live_since}<span class="live-note warn">{$t('robots.exec.armBodyLive')}</span>{/if}
                </div>
              {/if}
            {/if}
          {/if}
        {/if}
      </details>

      {#if (session.feed ?? 'hist') === 'hist'}
        <details class="sec">
          <summary class="sec-head">
            <span class="sec-title">
              <Icon name="chevron-right" size={12} />
              <h4>{$t('robots.exclusions.title')}</h4>
            </span>
            <span class="sec-value">{exclusionLabel}</span>
          </summary>
          <p class="sec-desc">{$t('robots.tickHintShort')} {$t('robots.exclusions.hint', { timezone: session.timezone })}</p>
          <ExclusionEditor bind:exclusions={session.exclusions} />
        </details>
        <details class="sec">
          <summary class="sec-head">
            <span class="sec-title">
              <Icon name="chevron-right" size={12} />
              <h4>{$t('robots.report.title')}</h4>
            </span>
            <span class="sec-value">{reportLabel}</span>
          </summary>
          <label class="check">
            <input type="checkbox" checked={session.cadence === 'scheduled'} onchange={(e) => (session.cadence = e.currentTarget.checked ? 'scheduled' : 'each')} />
            <span>{$t('robots.report.on')}<em>{$t('robots.report.hint')}</em></span>
          </label>
          {#if session.cadence === 'scheduled'}
            <SchedulePicker bind:rule={session} catchUp={false} />
          {/if}
        </details>
      {/if}

      <details class="sec">
        <summary class="sec-head">
          <span class="sec-title">
            <Icon name="chevron-right" size={12} />
            <h4>{$t('robots.window')}</h4>
          </span>
          <span class="sec-value">{windowLabel}</span>
        </summary>
        <div class="grid">
          <Input
            label={$t('robots.windowBars')}
            type="number"
            bind:value={session.window_bars}
            hint={$t('robots.windowHint')}
          />
        </div>
      </details>

      <details class="sec">
        <summary class="sec-head">
          <span class="sec-title">
            <Icon name="chevron-right" size={12} />
            <h4>{$t('robots.wizard.step.capital')}</h4>
          </span>
          <span class="sec-value">{Object.keys(capitalBody(session.capital)).length ? $t('robots.capital.set') : $t('robots.capital.noCap')}</span>
        </summary>
        <p class="sec-desc">{$t('robots.capital.hint')}</p>
        <CapitalEditor bind:capital={session.capital} broker={session.execution?.target === 'broker'} />
      </details>

      <details class="sec">
        <summary class="sec-head">
          <span class="sec-title">
            <Icon name="chevron-right" size={12} />
            <h4>{$t('robots.wizard.step.guards')}</h4>
          </span>
          <span class="sec-value">{$t('robots.wizard.guardCount', { n: session.guards?.length ?? 0 })}</span>
        </summary>
        <p class="sec-desc">{$t('robots.guard.hint')}</p>
        <GuardEditor bind:guards={session.guards} />
      </details>

      <details class="sec">
        <summary class="sec-head">
          <span class="sec-title">
            <Icon name="chevron-right" size={12} />
            <h4>{$t('robots.priority')}</h4>
          </span>
          <span class="sec-value">{session.priority ?? 100}</span>
        </summary>
        <div class="grid">
          <Input label={$t('robots.priority')} type="number" bind:value={session.priority} hint={$t('robots.priorityHint')} />
        </div>
      </details>

      <details class="sec">
        <summary class="sec-head">
          <span class="sec-title">
            <Icon name="chevron-right" size={12} />
            <h4>{$t('robots.reporting')}</h4>
          </span>
          <span class="sec-value">{reportingLabel}</span>
        </summary>
        <div class="sec-bar">
          <p class="sec-desc">{$t('robots.channelsHint')}</p>
          <ChannelButton module="robots" size="sm" onchanged={loadChannels} />
        </div>
        <div class="grid">
          <Select
            label={$t('robots.notifyWhen')}
            options={notifyOptions}
            bind:value={session.notify_when}
          />
          <div class="fld">
            <span class="lbl">{$t('robots.channels')}</span>
            <MultiSelect
              bind:value={session.channel_ids}
              options={channelOptions}
              allLabel={$t('robots.allChannels')}
              width="100%"
            />
          </div>
          <div class="fld">
            <span class="lbl">{$t('robots.instruments')}</span>
            <MultiSelect
              bind:value={session.notify_tickers}
              options={tickerOptions}
              allLabel={$t('robots.allInstruments')}
              width="100%"
            />
          </div>
          <!-- Which fills are worth a message. The log keeps everything either way, so
               this only decides what leaves for a channel. -->
          <div class="fld span2">
            <span class="lbl">{$t('robots.alertOn')}</span>
            <div class="checks">
              <label class="chk">
                <input type="checkbox" bind:checked={session.notify_open} />
                {$t('robots.onOpen')}
              </label>
              <label class="chk">
                <input type="checkbox" bind:checked={session.notify_close} />
                {$t('robots.onClose')}
              </label>
            </div>
          </div>
        </div>
      </details>

      <!-- Three messages, three wordings. Each tab owns its templates, its vocabulary
           and its preview: an entry cannot read the exit's numbers, and the summary
           reads no trade at all. -->
      <details class="sec">
        <summary class="sec-head">
          <span class="sec-title">
            <Icon name="chevron-right" size={12} />
            <h4>{$t('robots.message')}</h4>
          </span>
          <span class="sec-value">{messageLabel}</span>
        </summary>

        <div class="tabs" role="tablist">
          {#each kinds as k (k.id)}
            <button
              role="tab"
              aria-selected={msgKind === k.id}
              class:on={msgKind === k.id}
              onclick={() => (msgKind = k.id)}
            >
              {$t(`robots.kind.${k.id}`)}
              {#if session[k.title] || session[k.body]}<span class="mark">•</span>{/if}
            </button>
          {/each}
        </div>

        <div class="sec-bar">
          <p class="sec-desc">{$t(`robots.kindHint.${msgKind}`)}</p>
          <button class="link" onclick={() => (helperOpen = !helperOpen)}>
            <Icon name={helperOpen ? 'chevron-up' : 'chevron-down'} size={11} />
            {$t('robots.variables')}
          </button>
        </div>

        {#if helperOpen}
          <div class="helper">
            <p class="hint">{$t('robots.variablesHint')}</p>
            <div class="vars">
              {#each vars as v (v.path)}
                <button class="var" onclick={() => insert(v.path)} title={String(v.sample)}>
                  <code>{v.path}</code>
                  <span class="type">{v.type}</span>
                </button>
              {/each}
            </div>
            {#if filters.length}
              <p class="hint filters">
                {$t('robots.filters')}
                {#each filters as f (f)}<code class="f">| {f}</code>{/each}
              </p>
            {/if}
          </div>
        {/if}

        {#key msgKind}
          <div class="grid">
            <div class="span2">
              <Input
                label={$t('robots.titleTemplate')}
                bind:value={session[current.title]}
                placeholder={$t('robots.defaultWording')}
                onfocus={(e) => { focused = 'title'; focusedEl = e.currentTarget; }}
              />
            </div>
            <div class="span2">
              <Input
                label={$t('robots.bodyTemplate')}
                bind:value={session[current.body]}
                placeholder={$t('robots.defaultWording')}
                multiline
                rows={4}
                onfocus={(e) => { focused = 'body'; focusedEl = e.currentTarget; }}
              />
            </div>
          </div>
        {/key}

        <div class="preview">
          <div class="pv-head">
            <span>{$t('robots.preview')}</span>
            <span class="pv-kind">{$t(`robots.kind.${msgKind}`)}</span>
          </div>
          <div class="pv-body">
            <strong>{preview.title}</strong>
            <p>{preview.message}</p>
          </div>
        </div>
      </details>

      {#if error}<p class="err">{error}</p>{/if}
    </div>
  {/if}

  {#snippet footer()}
    <Button variant="ghost" onclick={() => (open = false)}>{$t('common.cancel')}</Button>
    <Button variant="primary" loading={saving} onclick={save}>{$t('common.save')}</Button>
  {/snippet}
</Modal>

<BackgroundTradingModal bind:open={signOpen} {account} onsigned={() => { signOpen = false; loadAccounts(); }} />

<style>
  /* One rhythm for the whole form: sections separated by a filet, a two-column grid
     inside each, and every label carried by the shared Field scaffolding. A control
     never spans the dialog unless its content does (a name, a message body). */
  .form {
    display: flex;
    flex-direction: column;
  }
  .sec {
    padding: var(--space-3) 0;
    border-top: var(--hairline) solid var(--border);
  }
  /* An open section breathes; a closed one is just its header line. */
  details.sec[open] > :global(:not(summary)) {
    margin-top: var(--space-3);
  }
  section.sec {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .sec.first {
    border-top: 0;
    padding-top: 0;
  }
  /* A section is a decision: closed it shows the answer, open it shows the controls.
     <details> carries the state, so nothing here needs script. */
  .sec-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    cursor: pointer;
    list-style: none;
    user-select: none;
  }
  .sec-head::-webkit-details-marker {
    display: none;
  }
  .sec-title {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--dim);
  }
  .sec-title :global(svg) {
    transition: transform var(--dur-fast) var(--ease);
  }
  details[open] > .sec-head .sec-title :global(svg) {
    transform: rotate(90deg);
  }
  .sec-head:hover .sec-title,
  .sec-head:hover .sec-value {
    color: var(--text);
  }
  h4 {
    margin: 0;
    font-size: var(--fs-metric-label);
    font-weight: var(--fw-medium);
    color: inherit;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  /* The answer, read without opening anything. */
  .sec-value {
    font-size: var(--fs-desc);
    color: var(--muted);
    text-align: right;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* The sentence that explains the whole section sits under its title, not adrift
     between two controls. */
  .sec-desc {
    margin: 0;
    font-size: var(--fs-desc);
    color: var(--dim);
    max-width: 62ch;
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
  .sec-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: var(--space-3);
    align-items: start;
  }
  .span2 {
    grid-column: 1 / -1;
  }
  /* Same shape as Field, for controls that are not Input/Select. */
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
    line-height: var(--lh-tight);
  }

  /* What is traded, read in one line: the lock, the instruments, the caveat. */
  .frozen {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--space-1) var(--space-3);
    border: var(--hairline) solid var(--border);
    background: var(--surface-2);
    padding: var(--space-2) var(--space-3);
  }
  .fz-label {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    font-size: var(--fs-desc);
    color: var(--dim);
  }
  /* The way out of a frozen block: the strategy is edited where it is built, not here. */
  .fz-edit {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    margin-left: auto;
    background: transparent;
    border: var(--hairline) solid var(--border-control);
    border-radius: 0;
    padding: 2px var(--space-2);
    color: var(--text);
    font-size: var(--fs-desc);
    cursor: pointer;
  }
  .fz-edit:hover {
    background: var(--surface);
  }
  .fz-orders {
    font-size: var(--fs-desc);
    color: var(--text);
  }
  .fz-hint {
    flex: 1 1 100%;
    font-size: var(--fs-desc);
    color: var(--dim);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
  }
  /* A ticker tag, not the global filter chip: square, static, ink in --text. */
  .chip {
    border: var(--hairline) solid var(--border-control);
    border-radius: 0;
    background: transparent;
    color: var(--text);
    cursor: default;
    padding: 1px var(--space-2);
    font-family: var(--mono);
    font-size: var(--fs-desc);
  }

  /* One frame, hairline dividers: a segmented control, not two loose buttons. */
  /* Same track as the message tabs: one frame, an inset pill for the answer. */
  .seg {
    display: inline-flex;
    align-items: stretch;
    gap: 2px;
    padding: 2px;
    border: var(--hairline) solid var(--border-control);
    width: fit-content;
  }
  .seg button {
    background: transparent;
    border: 0;
    border-radius: 0;
    padding: var(--space-1) var(--space-4);
    margin: 0;
    line-height: 1;
    color: var(--muted);
    font-size: var(--fs-body);
    cursor: pointer;
  }
  .seg button:hover:not(.on):not(:disabled) {
    color: var(--text);
  }
  .seg button.on {
    background: var(--surface-2);
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  .seg button:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .checks {
    display: flex;
    gap: var(--space-4);
    flex-wrap: wrap;
    min-height: var(--control-h);
    align-items: center;
  }
  .chk {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
    cursor: pointer;
  }

  /* One tab per message. The dot says the wording was written, so a session's three
     messages are readable without opening each one. */
  /* A track with a pill inside it: the group owns 2px of padding, so the selected fill
     is inset on every side and can never ride over the frame. */
  .tabs {
    display: inline-flex;
    align-items: stretch;
    gap: 2px;
    padding: 2px;
    border: var(--hairline) solid var(--border-control);
    width: fit-content;
  }
  .tabs button {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: transparent;
    border: 0;
    border-radius: 0;
    padding: var(--space-1) var(--space-4);
    margin: 0;
    line-height: 1;
    color: var(--muted);
    font-size: var(--fs-body);
    cursor: pointer;
  }
  .tabs button:hover:not(.on) {
    color: var(--text);
  }
  .tabs button.on {
    background: var(--surface-2);
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  .mark {
    color: var(--accent);
    line-height: 1;
  }
  .pv-kind {
    text-transform: none;
    letter-spacing: 0;
    color: var(--muted);
  }

  .hint {
    margin: 0;
    color: var(--dim);
    font-size: var(--fs-desc);
  }
  .link {
    background: none;
    border: 0;
    color: var(--text);
    text-decoration: underline;
    text-underline-offset: 2px;
    font-size: var(--fs-desc);
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
  }

  .helper {
    border: var(--hairline) solid var(--border);
    background: var(--surface-2);
    padding: var(--space-2);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .vars {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
    max-height: 160px;
    overflow-y: auto;
  }
  .var {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: var(--surface);
    border: var(--hairline) solid var(--border-control);
    border-radius: 0;
    padding: 2px var(--space-2);
    cursor: pointer;
    font-family: var(--mono);
    font-size: var(--fs-desc);
  }
  .var:hover {
    border-color: var(--muted);
    color: var(--text);
  }
  .var .type {
    color: var(--muted);
  }
  .filters {
    display: flex;
    gap: var(--space-1);
    flex-wrap: wrap;
    align-items: center;
  }
  .f {
    color: var(--muted);
  }

  /* The message as it will land, not as it is typed. */
  .preview {
    border: var(--hairline) solid var(--border);
  }
  .pv-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-1) var(--space-2);
    background: var(--surface-2);
    border-bottom: var(--hairline) solid var(--border);
    font-size: var(--fs-metric-label);
    color: var(--dim);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .pv-body {
    padding: var(--space-3);
    font-size: var(--text-sm);
  }
  .pv-body p {
    margin: var(--space-1) 0 0;
    white-space: pre-wrap;
    color: var(--muted);
  }
  .err {
    margin: var(--space-3) 0 0;
    color: var(--red);
    font-size: var(--text-sm);
  }

  @media (max-width: 640px) {
    .grid {
      grid-template-columns: minmax(0, 1fr);
    }
  }
  .live-note {
    font-size: var(--fs-desc);
    color: var(--dim);
    margin: var(--space-1) 0 0;
  }
  .live-note.warn {
    color: var(--amber);
  }
  .broker-value {
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  .exec-rows {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .exec-row,
  .exec-bg {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .exec-row .live-note,
  .exec-bg .live-note {
    margin: 0;
  }
  .chip.strong {
    color: var(--text);
    border-color: var(--muted);
  }
</style>
