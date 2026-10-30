<script>
  // Chart alerts: one series crossing another or a fixed level, and where the notification
  // goes. The list is the interesting half, because an alert is something you set once and
  // then need to be able to trust: each row says when it last fired, what it last saw, and
  // why it could not run when a connector or a quota got in the way.
  //
  // Two scopes: this chart's instrument (with the form), or every alert (the global list,
  // filtered, with pause-all). Both edit, pause, re-arm and delete in place, and the global
  // list opens an alert's instrument in the active chart.
  //
  // Things stated in the UI because they are decisions, not details:
  //   - a crossing is not a state ("above" waits for the series to come *through* the other
  //     one, so an alert placed under the price does not fire the instant it is created);
  //   - each side is a backtest operand (price, indicator, library indicator or script plot,
  //     metric, fixed value), the same vocabulary and engine as a strategy condition;
  //   - where it is watched: live while the app is open, on closed bars in the background,
  //     or live in the background (a three-notch slider, the live notches only when the
  //     provider streams this instrument at this timeframe);
  //   - how often it may fire, and until when (never expires by default).
  import { untrack } from 'svelte';
  import { goto } from '$app/navigation';
  import Icon from '$lib/ui/Icon.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import OperandEditor from '$lib/modules/backtest/OperandEditor.svelte';
  import ScriptInputs from '$lib/script/ScriptInputs.svelte';
  import { t } from '$lib/i18n';
  import { histvizApi } from './api.js';
  import { conditionLabel, isExpired, OP_KEYS } from './alertText.js';
  import { indicatorLib } from '$lib/indicators/api.js';
  import { channelsApi } from '$lib/notifications/api.js';
  import { settingsApi } from '$lib/settings/api.js';
  import { brokersApi } from '$lib/brokers/api.js';
  import BackgroundTradingModal from '$lib/brokers/BackgroundTradingModal.svelte';
  import SizeField from './SizeField.svelte';
  import OffsetField from './OffsetField.svelte';
  import TicketBox from './TicketBox.svelte';
  import {
    blankTrade,
    draftOf,
    tradeBody,
    tradeAccounts,
    tradeOptions,
    isLive,
    sameTrade,
    statusOf,
    statusWarns,
    TRADE_SIZINGS,
    FALLBACKS
  } from './alertTrade.js';

  let {
    open = $bindable(false),
    /** The pane's instrument; null = only the global list. */
    coords = null,
    /** Price the pane suggests: the selected horizontal line, or the last close. */
    suggested = null,
    /** A level asked for from the price scale's `+` ({ price, last }): a fresh alert at it,
     *  over whatever the form held. The direction follows the side of the price it is on.
     *  From the time axis' `+` ({ at }, ms): a fresh time alert at that instant. */
    level = null,
    connectorId = null,
    /** The provider streams this instrument at this timeframe: the live notches are open. */
    canLive = false,
    /** The chart's last price: what a trade on alert is previewed at. */
    price = null,
    /** Opened from the toolbar's "schedule trade": the form starts on its Trade section. */
    tradeFocus = false,
    onchanged
  } = $props();

  // ── Trade on alert ──
  // The alert's own form gains a Trade section: the order sent to a broker account when it
  // fires, priced from the price that fires it. Saved with the alert; armed on its own, once a
  // preview at the chart's price has shown every number.
  /** alert id -> its trade row. */
  let trades = $state({});
  /** Accounts a trade may go to: granted to the chart, trading on, the paper broker first. */
  let accounts = $state([]);
  let tradeOpen = $state(false);
  let tradeOn = $state(false);
  let draft = $state(blankTrade());
  let preview = $state(null);
  /** The body the last preview was asked for: arming needs the preview of what is armed. */
  let previewedBody = $state('');
  let previewBusy = $state(false);
  let tradeError = $state('');
  let tradeBusy = $state(false);
  let code = $state('');
  let codeAsked = $state(false);
  let signOpen = $state(false);
  let tradeEl = $state(null);
  /** The alert an arming waits on while background trading is being signed. */
  let pendingArm = $state(null);

  let alerts = $state([]);
  /** Library rows `{ id, name, kind, meta }`, `meta` = a script's compiled ScriptMeta. */
  let library = $state([]);
  let channels = $state([]);
  let sessions = $state([]);
  let error = $state('');
  let busy = $state(false);
  let scope = $state('chart');
  let filter = $state('all');

  // Draft of the alert being created.
  let lhs = $state({ kind: 'price', field: 'close' });
  let rhs = $state({ kind: 'const', value: 0 });
  let inputs = $state({}); // script inputs by indicator id
  let op = $state('above');
  let name = $state('');
  let modeIdx = $state(1); // index into MODES; background by default
  let frequency = $state('once');
  let cooldown = $state(3600);
  let sessionId = $state('');
  let picked = $state('all'); // 'all' = every channel the chart is granted; [] = in-app only
  let forever = $state(true);
  let expiry = $state(''); // datetime-local value
  let fireAt = $state(''); // datetime-local value, the instant a time alert fires at
  /** The alert the form edits; null = the form creates one on the pane's instrument. */
  let editing = $state(null);
  let formEl = $state(null);

  // The suggestion seeds the level once, when the modal opens. It is a live price, so
  // re-reading it would rewrite the field under the user's fingers every tick. And once the
  // level has been changed it is the user's: closing and reopening the form keeps it, until
  // the alert is created and the draft starts over from the price.
  let seeded = false;
  let touched = false;
  $effect(() => {
    if (!open) {
      seeded = false;
      return;
    }
    if (seeded) return;
    seeded = true;
    untrack(() => {
      scope = coords ? 'chart' : 'all';
      if (level?.at != null) {
        stopEditing();
        op = 'at_time';
        fireAt = localInput(new Date(level.at));
      } else if (level) {
        stopEditing();
        rhs = { kind: 'const', value: Number(level.price) };
        op = level.last != null && level.price < level.last ? 'below' : 'above';
      } else if (!touched) rhs = { kind: 'const', value: Number(suggested ?? 0) };
      if (tradeFocus) {
        tradeOpen = true;
        tradeOn = true;
        queueMicrotask(() => tradeEl?.scrollIntoView({ block: 'start', behavior: 'smooth' }));
      }
      load();
    });
  });

  async function load() {
    busy = true;
    try {
      alerts = await histvizApi.alerts(scope === 'chart' ? coords : null);
      error = '';
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
    indicatorLib
      .list()
      .then((rows) => (library = rows.map((r) => ({ id: r.id, name: r.name, kind: r.kind ?? 'dag', meta: r.meta?.script ?? null }))))
      .catch(() => {
        /* the library is optional: price alerts do not need it */
      });
    settingsApi
      .marketSessions()
      .then((r) => {
        sessions = r.sessions;
        if (!sessionId && sessions.length) sessionId = sessions[0].id;
      })
      .catch(() => {
        /* no session list: every other frequency still works */
      });
    channelsApi
      .list('histviz')
      .then((rows) => (channels = rows))
      .catch(() => {
        /* no channels granted: the in-app notification still fires */
      });
    await loadTrades();
  }

  async function loadTrades() {
    try {
      const [rows, list] = await Promise.all([histvizApi.alertTrades(), brokersApi.list('histviz')]);
      trades = Object.fromEntries(rows.map((r) => [r.alert_id, r]));
      accounts = tradeAccounts(list);
      if (!draft.accountId && accounts.length) draft.accountId = accounts[0].id;
    } catch {
      /* no broker module or no trading account: the Trade section says so */
    }
  }

  function setScope(s) {
    if (s === scope || (s === 'chart' && !coords)) return;
    scope = s;
    filter = 'all';
    load();
  }

  /** A date as a `datetime-local` value, in the browser's own time zone. */
  function localInput(d) {
    d.setSeconds(0, 0);
    return new Date(d.getTime() - d.getTimezoneOffset() * 60000).toISOString().slice(0, 16);
  }
  /** A month from now, the default when "never" is unticked. */
  const nextMonth = () => localInput(new Date(Date.now() + 30 * 86400000));
  $effect(() => {
    if (!forever && !expiry) expiry = nextMonth();
  });

  /** The scripts either side reads: their inputs are set here, once per script. */
  const scripts = $derived(
    [lhs, rhs]
      .filter((o) => o?.kind === 'custom_indicator')
      .map((o) => library.find((r) => r.id === o.id && r.kind === 'script'))
      .filter((r, i, all) => r && all.findIndex((x) => x?.id === r.id) === i)
  );

  /** Load an alert into the form. Saving it arms it again: an edit is a fresh start. */
  function edit(a) {
    editing = a;
    // The list is reactive state: structuredClone throws on its proxies.
    lhs = $state.snapshot(a.lhs);
    rhs = $state.snapshot(a.rhs);
    inputs = $state.snapshot(a.inputs ?? {});
    op = a.op;
    name = a.name ?? '';
    modeIdx = Math.max(0, MODES.indexOf(a.mode));
    frequency = a.frequency;
    cooldown = a.cooldown_secs || 3600;
    sessionId = a.session_id ?? sessionId;
    picked = channelIds(a);
    forever = !a.expires_at;
    expiry = a.expires_at ? localInput(new Date(a.expires_at)) : '';
    fireAt = a.fire_at ? localInput(new Date(a.fire_at)) : '';
    touched = true;
    error = '';
    const tr = trades[a.id];
    tradeOn = !!tr;
    tradeOpen = !!tr;
    draft = tr ? draftOf(tr) : blankTrade(accounts[0]?.id ?? '');
    resetTradeUi();
    queueMicrotask(() => formEl?.scrollIntoView({ block: 'start', behavior: 'smooth' }));
  }

  function resetTradeUi() {
    preview = null;
    previewedBody = '';
    tradeError = '';
    code = '';
    codeAsked = false;
  }

  function stopEditing() {
    tradeOn = false;
    tradeOpen = false;
    draft = blankTrade(accounts[0]?.id ?? '');
    resetTradeUi();
    editing = null;
    lhs = { kind: 'price', field: 'close' };
    rhs = { kind: 'const', value: Number(suggested ?? 0) };
    inputs = {};
    name = '';
    picked = 'all';
    forever = true;
    expiry = '';
    fireAt = '';
    touched = false;
  }

  /** The alert list's link: the alert's instrument in the active chart. */
  function openChart(a) {
    open = false;
    goto(`/histviz?alert=${a.id}`);
  }

  async function create() {
    const at = editing ?? (coords ? { ...coords, connector_id: connectorId } : null);
    if (!at) return;
    // The trade is checked before the alert is written: an alert never lands half saved.
    const trade = tradeOn ? tradeBody($state.snapshot(draft)) : null;
    if (trade?.error) {
      tradeOpen = true;
      tradeError = $t(`histviz.alerts.trade.err.${trade.error}`);
      return;
    }
    busy = true;
    try {
      const used = Object.fromEntries(scripts.map((r) => [r.id, inputs[r.id] ?? {}]));
      const body = {
        name: name.trim(),
        provider: at.provider,
        asset_type: at.asset_type,
        ticker: at.ticker,
        timeframe: at.timeframe,
        connector_id: at.connector_id ?? null,
        lhs: $state.snapshot(lhs),
        rhs: $state.snapshot(rhs),
        inputs: used,
        op,
        channels: picked,
        mode: MODES[modeIdx],
        frequency: tradeOn ? 'once' : frequency,
        cooldown_secs: !tradeOn && frequency === 'cooldown' ? Number(cooldown) || 0 : 0,
        session_id: !tradeOn && frequency === 'session' ? sessionId || null : null,
        expires_at: forever || !expiry ? null : new Date(expiry).toISOString(),
        fire_at: atTime && fireAt ? new Date(fireAt).toISOString() : null,
        enabled: true
      };
      const saved = editing ? await histvizApi.saveAlert(editing.id, body) : await histvizApi.createAlert(body);
      if (trade) {
        await histvizApi.saveAlertTrade(saved.id, trade.body);
      } else if (trades[saved.id]) {
        await histvizApi.deleteAlertTrade(saved.id);
      }
      error = '';
      // An alert with a trade stays in the form: its trade is previewed and armed next.
      if (trade) {
        await load();
        edit(saved);
        tradeOpen = true;
        onchanged?.();
        return;
      }
      stopEditing();
      await load();
      onchanged?.();
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }

  /** Re-arm a fired alert, or park a live one. Everything else about it is left alone. */
  async function toggle(a) {
    try {
      await histvizApi.saveAlert(a.id, { ...a, enabled: !a.enabled, channels: channelIds(a) });
      await load();
      onchanged?.();
    } catch (e) {
      error = e.message;
    }
  }

  async function remove(a) {
    try {
      await histvizApi.deleteAlert(a.id);
      await load();
      onchanged?.();
    } catch (e) {
      error = e.message;
    }
  }

  async function pauseAll() {
    try {
      await histvizApi.pauseAllAlerts();
      await load();
      onchanged?.();
    } catch (e) {
      error = e.message;
    }
  }

  // ── Trade on alert: preview, arm, disarm ──
  const tradeAccount = $derived(accounts.find((a) => a.id === draft.accountId) ?? null);
  const topts = $derived(tradeOptions(tradeAccount));
  $effect(() => {
    if (!topts.types.includes(draft.type)) draft.type = topts.types[0];
    if (draft.tif && !topts.tifs.includes(draft.tif)) draft.tif = '';
    if (!topts.exits && (draft.tpOn || draft.slOn)) draft.tpOn = draft.slOn = false;
    if (!topts.reduceOnly && draft.reduceOnly) draft.reduceOnly = false;
  });
  // An alert with a trade fires once.
  $effect(() => {
    if (tradeOn && frequency !== 'once') frequency = 'once';
  });
  const needsLimit = $derived(draft.type === 'limit' || draft.type === 'stop_limit');
  const needsStop = $derived(draft.type === 'stop' || draft.type === 'stop_limit');
  const savedTrade = $derived(editing ? (trades[editing.id] ?? null) : null);
  const draftBody = $derived(tradeBody($state.snapshot(draft)));
  /** The preview on screen is of the trade as it is now. */
  const previewFresh = $derived(!!preview?.ok && !draftBody.error && previewedBody === JSON.stringify(draftBody.body));
  /** Arming needs the preview of what is armed; arming again a trade that fired, unchanged
   *  since it was previewed and armed, is the list's one click. */
  const armReady = $derived(previewFresh || (!!savedTrade?.fired_at && sameTrade($state.snapshot(draft), savedTrade)));
  const accountOptions = $derived(
    accounts.map((a) => ({
      value: a.id,
      label: `${a.name}${a.broker === 'paper' ? ` · ${$t('histviz.alerts.trade.paper')}` : isLive(a) ? ` · ${$t('histviz.alerts.trade.live')}` : ''}`
    }))
  );
  const tradeSizings = $derived(TRADE_SIZINGS.map((v) => ({ value: v, label: $t(`histviz.alerts.trade.sizing.${v}`) })));
  const tradeTifs = $derived([
    { value: '', label: $t('histviz.alerts.trade.tifDefault') },
    ...topts.tifs.map((v) => ({ value: v, label: $t(`histviz.trade.tif.${v}`) }))
  ]);
  const fallbackOptions = $derived(FALLBACKS.map((v) => ({ value: v, label: $t(`histviz.alerts.trade.fallback.${v}`) })));
  const noBackground = $derived(!!tradeAccount && !tradeAccount.background_trading_ack);

  /** The order the trade would send at the chart's price, with the broker's dry run and what is
   *  already armed on the instrument. */
  async function previewTrade() {
    if (!editing || !(Number(price) > 0)) return;
    if (draftBody.error) {
      tradeError = $t(`histviz.alerts.trade.err.${draftBody.error}`);
      return;
    }
    previewBusy = true;
    tradeError = '';
    const asked = JSON.stringify(draftBody.body);
    try {
      preview = await histvizApi.previewAlertTrade(editing.id, price, draftBody.body);
      previewedBody = asked;
    } catch (e) {
      preview = null;
      tradeError = e.message;
    } finally {
      previewBusy = false;
    }
  }

  /** Arm an alert's trade: the form's draft is saved first when it changed. Without background
   *  trading on the account the signature opens, then arming goes on; with the trading lock on
   *  the code is asked and sent with the arming. An alert that fired is armed again with it. */
  async function armTrade(a, fromForm = false) {
    tradeError = '';
    tradeBusy = true;
    try {
      if (fromForm) {
        if (draftBody.error) throw new Error($t(`histviz.alerts.trade.err.${draftBody.error}`));
        if (!sameTrade($state.snapshot(draft), trades[a.id])) await histvizApi.saveAlertTrade(a.id, draftBody.body);
      }
      await histvizApi.armAlertTrade(a.id, code.trim());
      code = '';
      codeAsked = false;
      if (!a.enabled) await histvizApi.saveAlert(a.id, { ...a, enabled: true, channels: channelIds(a) });
      await load();
      if (editing?.id === a.id) editing = alerts.find((x) => x.id === a.id) ?? editing;
      onchanged?.();
    } catch (e) {
      if (e.code === 'background_trading_required') {
        pendingArm = { a, fromForm };
        signOpen = true;
      } else if (e.code === 'code_required') {
        if (editing?.id !== a.id) edit(a);
        tradeOpen = true;
        codeAsked = true;
        tradeError = '';
      } else {
        if (editing?.id !== a.id) error = e.message;
        tradeError = e.message;
      }
    } finally {
      tradeBusy = false;
    }
  }

  async function disarmTrade(a) {
    try {
      await histvizApi.disarmAlertTrade(a.id);
      await loadTrades();
    } catch (e) {
      error = e.message;
    }
  }

  /** Background trading was signed: reread the accounts, then finish the arming that asked. */
  async function onSigned() {
    await loadTrades();
    const p = pendingArm;
    pendingArm = null;
    if (p) await armTrade(p.a, p.fromForm);
  }
  const signAccount = $derived(
    accounts.find((x) => x.id === (pendingArm ? trades[pendingArm.a.id]?.account_id : null)) ?? tradeAccount
  );
  const lastText = (tr) => tr?.last_result?.message ?? tr?.last_result?.order ?? '';
  // The signature closed without signing: the arming that asked for it is dropped.
  $effect(() => {
    if (!signOpen) pendingArm = null;
  });

  /** An alert's channels as the form holds them: 'all', or the ids it is narrowed to. */
  const channelIds = (a) => (Array.isArray(a.channels) ? [...a.channels] : 'all');
  const sendsTo = (id) => picked === 'all' || picked.includes(id);

  /** Unticking one channel out of "all" keeps every other one. */
  function togglePick(id) {
    const ids = picked === 'all' ? channels.map((c) => c.id) : picked;
    picked = ids.includes(id) ? ids.filter((x) => x !== id) : [...ids, id];
  }

  const MODES = ['app', 'background', 'background_live'];
  const MODE_KEYS = {
    app: 'histviz.alerts.modeApp',
    background: 'histviz.alerts.modeBackground',
    background_live: 'histviz.alerts.modeBackgroundLive'
  };
  const FREQ_KEYS = {
    once: 'histviz.alerts.freqOnce',
    every: 'histviz.alerts.freqEvery',
    bar_close: 'histviz.alerts.freqBarClose',
    cooldown: 'histviz.alerts.freqCooldown',
    session: 'histviz.alerts.freqSession'
  };
  const FILTERS = {
    all: 'histviz.alerts.filterAll',
    armed: 'histviz.alerts.filterArmed',
    paused: 'histviz.alerts.filterPaused',
    expired: 'histviz.alerts.filterExpired',
    error: 'histviz.alerts.filterError'
  };

  // A provider that does not stream this leaves only the background notch. An alert on
  // another instrument is edited with every notch open: the server refuses a live mode its
  // provider cannot stream, with the reason.
  const liveOk = $derived(canLive || (!!editing && !sameInstrument(editing)));
  function sameInstrument(a) {
    return !!coords && a.provider === coords.provider && a.asset_type === coords.asset_type && a.ticker === coords.ticker && a.timeframe === coords.timeframe;
  }
  $effect(() => {
    if (!liveOk && modeIdx !== 1) modeIdx = 1;
  });
  const mode = $derived(MODES[modeIdx]);
  /** A time alert compares nothing: no sides, no mode, no frequency, no expiry. */
  const atTime = $derived(op === 'at_time');

  const opOptions = $derived(Object.entries(OP_KEYS).map(([v, k]) => ({ value: v, label: $t(k) })));
  const freqOptions = $derived(
    Object.entries(FREQ_KEYS).map(([v, k]) => ({
      value: v,
      label: $t(k),
      disabled: v === 'session' && !sessions.length
    }))
  );
  const sessionOptions = $derived(sessions.map((s) => ({ value: s.id, label: s.name })));

  // A script side needs a plot it may read; a side left without one cannot be created.
  const incomplete = $derived(
    [lhs, rhs].some((o) => o?.kind === 'custom_indicator' && !o.id) ||
      scripts.some((r) => [lhs, rhs].some((o) => o?.id === r.id && !o.plot))
  );

  const label = (a) => conditionLabel(a, library, $t);
  const when = (iso) => (iso ? new Date(iso).toLocaleString() : '');
  const armed = $derived(alerts.filter((a) => a.enabled).length);
  const matches = (a) =>
    filter === 'armed'
      ? a.enabled && !isExpired(a)
      : filter === 'paused'
        ? !a.enabled && !isExpired(a)
        : filter === 'expired'
          ? isExpired(a)
          : filter === 'error'
            ? !!a.last_error
            : true;
  const shown = $derived(alerts.filter(matches));
  // The global list reads by instrument: one heading per ticker and timeframe.
  const groups = $derived.by(() => {
    const out = new Map();
    for (const a of shown) {
      const key = `${a.provider}|${a.asset_type}|${a.ticker}|${a.timeframe}`;
      if (!out.has(key)) out.set(key, { key, title: `${a.ticker} · ${a.timeframe}`, sub: a.provider, rows: [] });
      out.get(key).rows.push(a);
    }
    return [...out.values()];
  });
</script>

<Modal bind:open title={$t('histviz.alerts.title')} size="lg">
  <ErrorText {error} />

  <div class="scopes" role="tablist">
    <button type="button" role="tab" class:on={scope === 'chart'} aria-selected={scope === 'chart'} disabled={!coords} onclick={() => setScope('chart')}>
      {coords ? `${coords.ticker} · ${coords.timeframe}` : $t('histviz.alerts.scopeChart')}
    </button>
    <button type="button" role="tab" class:on={scope === 'all'} aria-selected={scope === 'all'} onclick={() => setScope('all')}>
      {$t('histviz.alerts.scopeAll')}
    </button>
  </div>

  {#if editing || (scope === 'chart' && coords)}
    <p class="rule">{$t('histviz.alerts.rule')}</p>
    {#if editing}
      <p class="editing">
        <Icon name="pencil" size={12} />
        {$t('histviz.alerts.editing', { name: editing.name || editing.ticker, ticker: `${editing.ticker} · ${editing.timeframe}` })}
        <button type="button" class="chip" onclick={stopEditing}>{$t('common.cancel')}</button>
      </p>
    {/if}

    <form
      bind:this={formEl}
      class="new"
      onsubmit={(e) => {
        e.preventDefault();
        create();
      }}
    >
      <!-- `float` on every picker: the modal body scrolls its own overflow, and a scrolling
           ancestor clips an absolutely positioned menu down to a sliver. -->
      <div class="cond">
        <span class="lbl">{$t('histviz.alerts.condition')}</span>
        {#if atTime}
          <div class="side">
            <Dropdown bind:value={op} options={opOptions} ariaLabel={$t('histviz.alerts.op')} float />
          </div>
          <div class="side">
            <input type="datetime-local" bind:value={fireAt} aria-label={$t('histviz.alerts.fireAt')} />
          </div>
        {:else}
          <div class="side">
            <OperandEditor bind:operand={lhs} customIndicators={library} float />
          </div>
          <div class="side">
            <Dropdown bind:value={op} options={opOptions} ariaLabel={$t('histviz.alerts.op')} float />
          </div>
          <div class="side" oninput={() => (touched = true)}>
            <OperandEditor bind:operand={rhs} customIndicators={library} float />
          </div>
        {/if}
      </div>
      {#if atTime}
        <p class="hint">{$t('histviz.alerts.atTimeHint')}</p>
      {/if}

      {#each scripts as row (row.id)}
        <details class="inputs" open={scripts.length === 1}>
          <summary>{$t('histviz.alerts.scriptInputs', { name: row.name })}</summary>
          <ScriptInputs
            inputs={row.meta?.inputs ?? []}
            bind:values={() => inputs[row.id] ?? {}, (v) => (inputs = { ...inputs, [row.id]: v })}
          />
        </details>
      {/each}
      {#if scripts.length && !atTime}
        <p class="hint">{$t('histviz.alerts.scriptAtClose')}</p>
      {/if}

      {#if !atTime}
        <div class="modes">
          <span class="lbl">{$t('histviz.alerts.mode')}</span>
          <input
            type="range"
            min="0"
            max="2"
            step="1"
            bind:value={modeIdx}
            disabled={!liveOk}
            aria-label={$t('histviz.alerts.mode')}
            aria-valuetext={$t(MODE_KEYS[mode])}
          />
          <div class="notches">
            {#each MODES as m, i (m)}
              <button
                type="button"
                class="notch"
                class:on={modeIdx === i}
                disabled={!liveOk && i !== 1}
                onclick={() => (modeIdx = i)}
              >
                {$t(MODE_KEYS[m])}
              </button>
            {/each}
          </div>
          <p class="hint">
            {liveOk ? $t(`${MODE_KEYS[mode]}Hint`) : $t('histviz.alerts.noLive')}
          </p>
        </div>
      {/if}

      <div class="row">
        {#if !atTime}
          <div class="f">
            <span>{$t('histviz.alerts.frequency')}</span>
            <Dropdown
              bind:value={frequency}
              options={freqOptions}
              ariaLabel={$t('histviz.alerts.frequency')}
              title={tradeOn ? $t('histviz.alerts.trade.onceHint') : ''}
              disabled={tradeOn}
              float
            />
          </div>
          {#if frequency === 'cooldown'}
            <label class="f">
              {$t('histviz.alerts.cooldown')}
              <input type="number" min="1" step="60" bind:value={cooldown} />
            </label>
          {:else if frequency === 'session'}
            <div class="f">
              <span>
                {$t('histviz.alerts.session')}
                <a class="edit" href="/settings#sessions">{$t('histviz.alerts.sessionsLink')}</a>
              </span>
              <Dropdown
                bind:value={sessionId}
                options={sessionOptions}
                ariaLabel={$t('histviz.alerts.session')}
                float
              />
            </div>
          {/if}
          <div class="f">
            <span>{$t('histviz.alerts.expiry')}</span>
            <span class="expiry">
              <label class="chk">
                <input type="checkbox" bind:checked={forever} />
                {$t('histviz.alerts.expiryNever')}
              </label>
              {#if !forever}
                <input type="datetime-local" bind:value={expiry} aria-label={$t('histviz.alerts.expiry')} />
              {/if}
            </span>
          </div>
        {/if}
        <label class="f wide">
          {$t('histviz.alerts.name')}
          <input bind:value={name} placeholder={editing?.ticker ?? coords?.ticker ?? ''} />
        </label>
        <button
          class="add"
          type="submit"
          disabled={busy ||
            (atTime
              ? !fireAt
              : incomplete ||
                (!forever && !expiry) ||
                (frequency === 'session' && !sessionId) ||
                (frequency === 'cooldown' && !(Number(cooldown) > 0)))}
        >
          <Icon name={editing ? 'check' : 'bell'} size={13} /> {$t(editing ? 'histviz.alerts.save' : 'histviz.alerts.create')}
        </button>
      </div>

      {#if channels.length}
        <div class="chans">
          <span class="lbl">{$t('histviz.alerts.channels')}</span>
          <label class="chk">
            <input
              type="checkbox"
              checked={picked === 'all'}
              onchange={() => (picked = picked === 'all' ? [] : 'all')}
            />
            {$t('histviz.alerts.channelsAll')}
          </label>
          {#each channels as c (c.id)}
            <label class="chk">
              <input
                type="checkbox"
                checked={sendsTo(c.id)}
                onchange={() => togglePick(c.id)}
              />
              {c.name}
            </label>
          {/each}
          <span class="hint">{$t('histviz.alerts.channelsHint')}</span>
        </div>
      {:else}
        <p class="hint">
          {$t('histviz.alerts.noChannels')}
          <a href="/settings">{$t('histviz.alerts.noChannelsLink')}</a>
        </p>
      {/if}

      <!-- Trade on alert: collapsed until asked for, laid out like the order ticket. -->
      <section class="trade" bind:this={tradeEl} aria-label={$t('histviz.alerts.trade.section')}>
        <button type="button" class="trade-h" aria-expanded={tradeOpen} onclick={() => (tradeOpen = !tradeOpen)}>
          <Icon name="timer" size={12} />
          <span>{$t('histviz.alerts.trade.section')}</span>
          {#if savedTrade}
            {@const st = statusOf(savedTrade)}
            <span class="tchip" class:on={st === 'armed'} class:warn={statusWarns(st)}>{$t(`histviz.alerts.trade.status.${st}`)}</span>
          {/if}
          <span class="grow"></span>
          <Icon name={tradeOpen ? 'chevron-up' : 'chevron-down'} size={12} />
        </button>
        {#if tradeOpen}
          <div class="trade-body">
            {#if !accounts.length}
              <p class="hint">{$t('histviz.alerts.trade.noAccounts')} <a href="/settings#brokers">{$t('histviz.alerts.trade.noAccountsLink')}</a></p>
            {:else}
              <label class="chk on-row">
                <input type="checkbox" bind:checked={tradeOn} />
                {$t('histviz.alerts.trade.on')}
              </label>
              {#if tradeOn}
                <p class="hint">{$t('histviz.alerts.trade.riskNote')}</p>
                <div class="tgrid">
                  <div class="tfld">
                    <span class="lab">{$t('histviz.alerts.trade.account')}</span>
                    <Dropdown bind:value={draft.accountId} options={accountOptions} ariaLabel={$t('histviz.alerts.trade.account')} float />
                  </div>
                  <div class="tfld">
                    <span class="lab">{$t('histviz.alerts.trade.intent')}</span>
                    <div class="seg" role="radiogroup" aria-label={$t('histviz.alerts.trade.intent')}>
                      {#each ['enter', 'exit'] as v (v)}
                        <button type="button" role="radio" aria-checked={draft.intent === v} class:on={draft.intent === v} onclick={() => (draft.intent = v)}>
                          {$t(`histviz.alerts.trade.intents.${v}`)}
                        </button>
                      {/each}
                    </div>
                  </div>
                </div>
                {#if noBackground}
                  <p class="note warn">
                    <Icon name="alert-triangle" size={12} />
                    <span>{$t('histviz.alerts.trade.background')}</span>
                    <button type="button" class="chip" onclick={() => (signOpen = true)}>{$t('histviz.alerts.trade.sign')}</button>
                  </p>
                {/if}

                {#if draft.intent === 'exit'}
                  <p class="hint">{$t('histviz.alerts.trade.exitHint')}</p>
                {:else}
                  <div class="tgrid">
                    <div class="tfld">
                      <span class="lab">{$t('histviz.trade.side')}</span>
                      <div class="seg sides" role="radiogroup" aria-label={$t('histviz.trade.side')}>
                        <button type="button" role="radio" aria-checked={draft.side === 'sell'} class="sell" class:on={draft.side === 'sell'} onclick={() => (draft.side = 'sell')}>
                          {$t('histviz.trade.sell')}
                        </button>
                        <button type="button" role="radio" aria-checked={draft.side === 'buy'} class="buy" class:on={draft.side === 'buy'} onclick={() => (draft.side = 'buy')}>
                          {$t('histviz.trade.buy')}
                        </button>
                      </div>
                    </div>
                    <div class="tfld">
                      <span class="lab">{$t('histviz.trade.typeLabel')}</span>
                      <div class="seg" role="radiogroup" aria-label={$t('histviz.trade.typeLabel')}>
                        {#each topts.types as v (v)}
                          <button type="button" role="radio" aria-checked={draft.type === v} class:on={draft.type === v} onclick={() => (draft.type = v)}>
                            {$t(`histviz.trade.type.${v}`)}
                          </button>
                        {/each}
                      </div>
                    </div>
                  </div>
                  <div class="tgrid">
                    <SizeField
                      label={$t('histviz.trade.qty')}
                      bind:sizing={draft.sizing}
                      bind:qty={draft.qty}
                      bind:amount={draft.amount}
                      options={tradeSizings}
                      sized={previewFresh ? $t('histviz.trade.sizedUnits', { n: preview.qty }) : ''}
                      amountHint={draft.sizing === 'units' ? '' : $t(`histviz.alerts.trade.sizingHint.${draft.sizing}`)}
                      amountLabel={$t(`histviz.alerts.trade.sizing.${draft.sizing}`)}
                    />
                    {#if needsLimit}
                      <OffsetField
                        label={$t('histviz.trade.limitPrice')}
                        bind:offset={draft.limit}
                        hint={$t('histviz.alerts.trade.entryHint')}
                        note={previewFresh && preview.limit ? preview.limit : ''}
                      />
                    {/if}
                    {#if needsStop}
                      <OffsetField
                        label={$t('histviz.trade.stopPrice')}
                        bind:offset={draft.stop}
                        hint={$t('histviz.alerts.trade.entryHint')}
                        note={previewFresh && preview.stop ? preview.stop : ''}
                      />
                    {/if}
                    {#if topts.exits}
                      <OffsetField
                        label={$t('histviz.trade.takeProfit')}
                        bind:offset={draft.tp}
                        switchable
                        bind:on={draft.tpOn}
                        hint={$t('histviz.alerts.trade.exitsHint')}
                        note={previewFresh && preview.take_profit ? preview.take_profit : ''}
                      />
                      <OffsetField
                        label={$t('histviz.trade.stopLoss')}
                        bind:offset={draft.sl}
                        switchable
                        bind:on={draft.slOn}
                        hint={$t('histviz.alerts.trade.exitsHint')}
                        note={previewFresh && preview.stop_loss ? preview.stop_loss : ''}
                      />
                    {/if}
                    <div class="tfld">
                      <span class="lab">{$t('histviz.trade.tifLabel')}</span>
                      <Dropdown bind:value={draft.tif} options={tradeTifs} ariaLabel={$t('histviz.trade.tifLabel')} float />
                    </div>
                    {#if topts.reduceOnly}
                      <label class="chk tf-chk" title={$t('histviz.trade.reduceOnlyHint')}>
                        <input type="checkbox" bind:checked={draft.reduceOnly} />
                        {$t('histviz.trade.reduceOnly')}
                      </label>
                    {/if}
                  </div>
                {/if}

                <div class="tgrid">
                  <div class="tfld" title={$t('histviz.alerts.trade.fallbackHint')}>
                    <span class="lab">{$t('histviz.alerts.trade.fallbackLabel')}</span>
                    <Dropdown bind:value={draft.fallback} options={fallbackOptions} ariaLabel={$t('histviz.alerts.trade.fallbackLabel')} float />
                  </div>
                </div>

                <!-- Every number shown before arming: the order at the chart's price, the
                     broker's dry run, what is already armed on the instrument. -->
                <div class="pv">
                  {#if !editing}
                    <p class="hint">{$t('histviz.alerts.trade.previewNeedsAlert')}</p>
                  {:else if !(Number(price) > 0)}
                    <p class="hint">{$t('histviz.alerts.trade.previewNeedsPrice')}</p>
                  {:else}
                    <button type="button" class="chip" disabled={previewBusy} onclick={previewTrade}>
                      <Icon name="eye" size={11} />
                      {previewBusy ? $t('histviz.alerts.trade.previewing') : $t('histviz.alerts.trade.preview')}
                    </button>
                  {/if}
                  {#if preview}
                    {#if !preview.ok}
                      <p class="note warn"><Icon name="alert-triangle" size={12} /> <span>{preview.message}</span></p>
                    {:else if preview.intent === 'exit'}
                      <p class="pv-line">
                        {$t('histviz.alerts.trade.exitNow', {
                          positions: (preview.exit?.positions ?? []).map((p) => `${p.side} ${p.qty} ${p.symbol}`).join(', ') || $t('histviz.alerts.trade.nothingHeld'),
                          orders: preview.exit?.orders ?? 0
                        })}
                      </p>
                    {:else}
                      <p class="pv-line mono" class:stale={!previewFresh}>{$t('histviz.alerts.trade.previewAt', { price: String(price), order: preview.order })}</p>
                      {#if preview.dry_run}
                        <p class="pv-line" class:bad={!preview.dry_run.ok}>
                          {preview.dry_run.ok
                            ? $t('histviz.alerts.trade.dryRunOk', { detail: preview.dry_run.detail })
                            : $t('histviz.alerts.trade.dryRunNo', { detail: preview.dry_run.detail })}
                        </p>
                      {/if}
                    {/if}
                    {#if preview.exposure}
                      {@const x = preview.exposure}
                      <p class="pv-line">
                        {x.count
                          ? $t('histviz.alerts.trade.exposure', { ticker: editing?.ticker ?? '', n: x.count, buy: x.buy, sell: x.sell })
                          : $t('histviz.alerts.trade.exposureNone', { ticker: editing?.ticker ?? '' })}
                        {#if x.exits}{$t('histviz.alerts.trade.exposureExits', { n: x.exits })}{/if}
                        {#if x.unpriced}{$t('histviz.alerts.trade.exposureUnpriced', { n: x.unpriced })}{/if}
                      </p>
                    {/if}
                  {/if}
                </div>

                {#if savedTrade && lastText(savedTrade)}
                  <p class="hint">{$t('histviz.alerts.trade.lastFire', { text: lastText(savedTrade) })}</p>
                {/if}
                {#if codeAsked}
                  <div class="tgrid">
                    <div class="tfld">
                      <span class="lab">{$t('histviz.alerts.trade.code')}</span>
                      <TicketBox>
                        <input type="text" bind:value={code} inputmode="numeric" autocomplete="one-time-code" maxlength="10" aria-label={$t('histviz.alerts.trade.code')} onkeydown={(e) => { if (e.key === 'Enter') { e.preventDefault(); armTrade(editing, true); } }} />
                      </TicketBox>
                      <span class="hint">{$t('histviz.alerts.trade.codeHint')}</span>
                    </div>
                  </div>
                {/if}
                {#if tradeError}<p class="note warn"><Icon name="alert-triangle" size={12} /> <span>{tradeError}</span></p>{/if}
                {#if editing}
                  <div class="tact">
                    {#if savedTrade?.armed_at && sameTrade($state.snapshot(draft), savedTrade)}
                      <span class="tchip on">{$t('histviz.alerts.trade.armedSince', { when: when(savedTrade.armed_at) })}</span>
                      <button type="button" class="chip" onclick={() => disarmTrade(editing)}>{$t('histviz.alerts.trade.disarm')}</button>
                    {:else}
                      <button
                        type="button"
                        class="add arm"
                        disabled={tradeBusy || !armReady || (codeAsked && !code.trim())}
                        title={armReady ? '' : $t('histviz.alerts.trade.armNeedsPreview')}
                        onclick={() => armTrade(editing, true)}
                      >
                        <Icon name="zap" size={13} />
                        {$t(savedTrade?.fired_at ? 'histviz.alerts.trade.rearm' : 'histviz.alerts.trade.arm')}
                      </button>
                      {#if !armReady}<span class="hint">{$t('histviz.alerts.trade.armNeedsPreview')}</span>{/if}
                    {/if}
                  </div>
                {/if}
              {/if}
            {/if}
          </div>
        {/if}
      </section>
    </form>
  {/if}

  <div class="head">
    <h4 class="sec">{$t('histviz.alerts.existing', { armed, total: alerts.length })}</h4>
    <div class="filters">
      {#each Object.entries(FILTERS) as [f, k] (f)}
        <button type="button" class="chip" class:on={filter === f} onclick={() => (filter = f)}>{$t(k)}</button>
      {/each}
      {#if scope === 'all' && armed > 0}
        <button type="button" class="chip" onclick={pauseAll}>
          <Icon name="pause" size={11} /> {$t('histviz.alerts.pauseAll')}
        </button>
      {/if}
    </div>
  </div>

  {#snippet item(a)}
    {@const expired = isExpired(a)}
    <li class:off={!a.enabled || expired}>
      <span class="dot" class:on={a.enabled && !expired}></span>
      <span class="what">
        <strong>{a.name || a.ticker}</strong>
        <span class="cond-text">{label(a)}</span>
        {#if scope === 'chart'}<span class="tf">{a.timeframe}</span>{/if}
        {#if a.op !== 'at_time'}
          <span class="badge" class:live={a.mode !== 'background'}>{$t(MODE_KEYS[a.mode] ?? MODE_KEYS.background)}</span>
          <span class="tf">{$t(FREQ_KEYS[a.frequency] ?? FREQ_KEYS.once)}</span>
        {/if}
        {#if expired}
          <span class="badge">{$t('histviz.alerts.expired')}</span>
        {:else if a.expires_at}
          <span class="tf">{$t('histviz.alerts.expiresAt', { when: when(a.expires_at) })}</span>
        {/if}
        {#if trades[a.id]}
          {@const st = statusOf(trades[a.id])}
          <span class="badge trade-b">{$t('histviz.alerts.trade.badge')}</span>
          <span class="tchip" class:on={st === 'armed'} class:warn={statusWarns(st)} title={lastText(trades[a.id])}>{$t(`histviz.alerts.trade.status.${st}`)}</span>
        {/if}
      </span>
      <span class="state">
        {#if a.last_error}
          <span class="warn" title={a.last_error}>
            <Icon name="alert-triangle" size={11} /> {$t('histviz.alerts.cannotRun')}
          </span>
        {:else if a.last_fired_at}
          {$t('histviz.alerts.firedAt', { when: when(a.last_fired_at) })}
        {:else if a.last_value != null}
          {$t('histviz.alerts.watching', { value: a.last_value })}
        {:else}
          {$t('histviz.alerts.pending')}
        {/if}
      </span>
      {#if scope === 'all'}
        <button class="icon" title={$t('histviz.alerts.openChart')} onclick={() => openChart(a)}>
          <Icon name="candlestick" size={12} />
        </button>
      {/if}
      <button class="icon" class:on={editing?.id === a.id} title={$t('histviz.alerts.edit')} onclick={() => edit(a)}>
        <Icon name="pencil" size={12} />
      </button>
      {#if trades[a.id]}
        {#if trades[a.id].armed_at}
          <button class="icon on" title={$t('histviz.alerts.trade.disarm')} aria-label={$t('histviz.alerts.trade.disarm')} onclick={() => disarmTrade(a)}>
            <Icon name="zap" size={12} />
          </button>
        {:else}
          <button class="icon" disabled={tradeBusy} title={$t('histviz.alerts.trade.rearm')} aria-label={$t('histviz.alerts.trade.rearm')} onclick={() => armTrade(a)}>
            <Icon name="zap" size={12} />
          </button>
        {/if}
      {/if}
      <button
        class="icon"
        disabled={expired && !a.enabled}
        title={expired && !a.enabled
          ? $t('histviz.alerts.expiredHint')
          : $t(a.enabled ? 'histviz.alerts.pause' : 'histviz.alerts.arm')}
        onclick={() => toggle(a)}
      >
        <Icon name={a.enabled ? 'pause' : 'play'} size={12} />
      </button>
      <button class="icon danger" title={$t('common.remove')} onclick={() => remove(a)}>
        <Icon name="trash-2" size={12} />
      </button>
    </li>
  {/snippet}

  {#if scope === 'all'}
    {#each groups as g (g.key)}
      <h5 class="group">{g.title} <span class="tf">{g.sub}</span></h5>
      <ul class="list">
        {#each g.rows as a (a.id)}{@render item(a)}{/each}
      </ul>
    {:else}
      <p class="empty">{busy ? $t('common.loading') : $t('histviz.alerts.emptyAll')}</p>
    {/each}
  {:else}
    <ul class="list">
      {#each shown as a (a.id)}
        {@render item(a)}
      {:else}
        <li class="empty">{busy ? $t('common.loading') : $t('histviz.alerts.empty')}</li>
      {/each}
    </ul>
  {/if}
</Modal>

<BackgroundTradingModal bind:open={signOpen} account={signAccount} onsigned={onSigned} />

<style>
  .scopes {
    display: flex;
    gap: var(--space-1);
    margin-bottom: var(--space-2);
    border-bottom: 1px solid var(--border);
  }
  .scopes button {
    background: none;
    border: 0;
    border-bottom: 2px solid transparent;
    padding: var(--space-1) var(--space-2);
    color: var(--muted);
    cursor: pointer;
    font-size: 0.78rem;
  }
  .scopes button.on {
    color: var(--text);
    border-bottom-color: var(--accent);
  }
  .scopes button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .cond {
    display: grid;
    grid-template-columns: 90px 1fr;
    gap: var(--space-1) var(--space-2);
    align-items: center;
  }
  .cond .lbl {
    grid-row: span 3;
    align-self: start;
    padding-top: var(--space-1);
    font-size: 0.72rem;
    color: var(--muted);
  }
  .side {
    display: flex;
    min-width: 0;
  }
  .inputs summary {
    cursor: pointer;
    font-size: 0.74rem;
    color: var(--muted);
  }
  .expiry {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    height: var(--control-h);
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .filters {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 1px var(--space-2);
    background: none;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--muted);
    font-size: 0.7rem;
    cursor: pointer;
  }
  .chip.on {
    color: var(--text);
    border-color: var(--accent);
  }
  .group {
    margin: var(--space-3) 0 0;
    font-size: 0.78rem;
  }
  .empty {
    color: var(--muted);
    font-size: 0.76rem;
  }
  .editing {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: 0 0 var(--space-2);
    font-size: 0.76rem;
  }
  .icon.on {
    color: var(--accent);
    border-color: var(--accent);
  }
  .rule {
    margin: 0 0 var(--space-2);
    font-size: 0.74rem;
    color: var(--muted);
  }
  .new {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding-bottom: var(--space-3);
    border-bottom: 1px solid var(--border);
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    align-items: flex-end;
  }
  .f {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: 0.72rem;
    color: var(--muted);
    min-width: 90px;
  }
  .f.wide {
    flex: 1;
    min-width: 160px;
  }
  .chk {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 0.74rem;
    color: var(--muted);
  }
  .add {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: var(--control-h);
    padding: 0 var(--space-3);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    white-space: nowrap;
    color: var(--text);
    cursor: pointer;
  }
  .add:hover:not(:disabled) {
    border-color: var(--accent);
  }
  .add:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .modes {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .modes input[type='range'] {
    width: 100%;
    accent-color: var(--accent);
  }
  .notches {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
  }
  .notch {
    background: none;
    border: 0;
    padding: 0;
    font-size: 0.72rem;
    color: var(--muted);
    cursor: pointer;
  }
  .notch:nth-child(1) {
    text-align: left;
  }
  .notch:nth-child(3) {
    text-align: right;
  }
  .notch.on {
    color: var(--text);
    font-weight: 600;
  }
  .notch:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .modes .lbl {
    font-size: 0.72rem;
    color: var(--muted);
  }
  .modes .hint {
    margin: 0;
  }
  .edit {
    margin-left: var(--space-1);
    font-size: 0.68rem;
  }
  .badge {
    font-size: 0.66rem;
    padding: 0 var(--space-1);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--muted);
  }
  .badge.live {
    color: var(--accent);
    border-color: var(--accent);
  }
  .chans {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }
  .chans .lbl,
  .hint {
    font-size: 0.72rem;
    color: var(--muted);
  }
  .sec {
    margin: var(--space-3) 0 var(--space-1);
    font-size: 0.8rem;
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .list li {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) 0;
    border-bottom: 1px solid var(--border);
    font-size: 0.76rem;
  }
  .list li.off {
    opacity: 0.6;
  }
  .list li.empty {
    color: var(--muted);
    border: 0;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--muted);
  }
  .dot.on {
    background: var(--green);
  }
  .what {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
  }
  .cond-text {
    color: var(--muted);
  }
  .tf {
    color: var(--muted);
    font-size: 0.7rem;
  }
  .state {
    color: var(--muted);
    font-size: 0.7rem;
    white-space: nowrap;
  }
  .warn {
    color: var(--amber);
    display: inline-flex;
    align-items: center;
    gap: 3px;
  }
  .icon {
    display: inline-flex;
    padding: 3px;
    background: none;
    border: 1px solid transparent;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
  }
  .icon:hover:not(:disabled) {
    color: var(--text);
    border-color: var(--border);
  }
  .icon:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .icon.danger:hover {
    color: var(--red);
    border-color: var(--red);
  }

  /* ── Trade on alert: calm and compact, like the order ticket ── */
  .trade {
    border-top: 1px solid var(--border);
    padding-top: var(--space-2);
  }
  .trade-h {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    padding: 0;
    border: 0;
    background: none;
    color: var(--text);
    font-size: 0.78rem;
    cursor: pointer;
  }
  .grow {
    flex: 1;
  }
  .trade-body {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    margin-top: var(--space-2);
    font-size: 0.74rem;
  }
  .trade-body .hint {
    margin: 0;
  }
  .on-row {
    color: var(--text);
  }
  .tgrid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(170px, 1fr));
    gap: var(--space-2) var(--space-3);
    align-items: end;
  }
  .tfld {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }
  .tfld .lab {
    color: var(--muted);
  }
  .tf-chk {
    align-self: end;
    height: var(--control-h);
  }
  /* A row of choices sharing one frame: the picked one is underlined, as on the ticket. */
  .seg {
    display: flex;
    height: var(--control-h);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
    overflow: hidden;
  }
  .seg button {
    flex: 1;
    min-width: 0;
    padding: 0 var(--space-1);
    border: 0;
    border-bottom: 2px solid transparent;
    background: var(--surface);
    color: var(--muted);
    font-size: 0.72rem;
    cursor: pointer;
    white-space: nowrap;
  }
  .seg button.on {
    color: var(--text);
    border-bottom-color: var(--accent);
  }
  .sides button.buy.on {
    color: var(--green);
    border-bottom-color: var(--green);
  }
  .sides button.sell.on {
    color: var(--red);
    border-bottom-color: var(--red);
  }
  .note {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: 0;
  }
  .note.warn {
    display: flex;
  }
  .pv {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-1);
  }
  .pv-line {
    margin: 0;
    color: var(--muted);
  }
  .pv-line.mono {
    font-family: var(--mono);
    color: var(--text);
  }
  .pv-line.stale {
    opacity: 0.55;
  }
  .pv-line.bad {
    color: var(--amber);
  }
  .tact {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }
  .add.arm:disabled {
    opacity: 0.5;
  }
  .tchip {
    font-size: 0.66rem;
    padding: 0 var(--space-1);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--muted);
    white-space: nowrap;
  }
  .tchip.on {
    color: var(--green);
    border-color: var(--green);
  }
  .tchip.warn {
    color: var(--amber);
    border-color: var(--amber);
  }
  .trade-b {
    color: var(--accent);
    border-color: var(--accent);
  }
</style>
