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
  //     or live in the background (three cards, the live ones only when the
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
  import { connectorsApi } from '$lib/connectors/api.js';
  import BackgroundTradingModal from '$lib/brokers/BackgroundTradingModal.svelte';
  import BrokersModal from '$lib/brokers/BrokersModal.svelte';
  import ChannelsModal from '$lib/notifications/ChannelsModal.svelte';
  import MarketSessionsSection from '$lib/settings/MarketSessionsSection.svelte';
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
     *  From the time axis' `+` ({ at }, ms): a fresh time alert at that instant.
     *  From an alert's mark on a scale ({ edit }, its id): that alert, in the form. */
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
  /** connector id -> its name, for the source on each row. */
  let connectorNames = $state({});
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
  let modeIdx = $state(0); // index into MODES; app (live feed) by default
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
      const target = level?.edit ?? null;
      if (target) {
        // Nothing to seed: the form is about to hold this alert.
      } else if (level?.at != null) {
        stopEditing();
        op = 'at_time';
        fireAt = localInput(new Date(level.at), true);
      } else if (level) {
        stopEditing();
        rhs = { kind: 'const', value: Number(level.price) };
        op = level.last != null && level.price < level.last ? 'below' : 'above';
      } else if (!touched) rhs = { kind: 'const', value: Number(suggested ?? 0) };
      if (tradeFocus) {
        tradeOn = true;
        queueMicrotask(() => tradeEl?.scrollIntoView({ block: 'start', behavior: 'smooth' }));
      }
      // After the load: the form reads the alert's trade from it.
      load()
        .then(() => (target ? histvizApi.alert(target).then(edit) : null))
        .catch((e) => (error = e.message));
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
    loadSessions();
    loadChannels();
    connectorsApi
      .list()
      .then((rows) => (connectorNames = Object.fromEntries(rows.map((c) => [c.id, c.name]))))
      .catch(() => {
        /* no broker list: a row names its provider instead */
      });
    await loadTrades();
  }

  // A missing session, channel or account is set up in a modal over this one (siblings of
  // it, outside the form), so the half-filled alert stays as typed; each list is reread
  // when its modal reports a change.
  let sessionsOpen = $state(false);
  let channelsOpen = $state(false);
  let brokersOpen = $state(false);

  function loadSessions() {
    settingsApi
      .marketSessions()
      .then((r) => {
        sessions = r.sessions;
        if (!sessions.some((s) => s.id === sessionId)) sessionId = sessions[0]?.id ?? '';
      })
      .catch(() => {
        /* no session list: every other frequency still works */
      });
  }

  function loadChannels() {
    channelsApi
      .list('histviz')
      .then((rows) => (channels = rows))
      .catch(() => {
        /* no channels granted: the in-app notification still fires */
      });
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

  /** A date as a `datetime-local` value, in the browser's own time zone: to the minute, or
   * to the second (`seconds`) for the instant a time alert fires at. */
  function localInput(d, seconds = false) {
    if (seconds) d.setMilliseconds(0);
    else d.setSeconds(0, 0);
    return new Date(d.getTime() - d.getTimezoneOffset() * 60000).toISOString().slice(0, seconds ? 19 : 16);
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
    fireAt = a.fire_at ? localInput(new Date(a.fire_at), true) : '';
    touched = true;
    error = '';
    const tr = trades[a.id];
    tradeOn = !!tr;
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
    // Read the pending arming before any await: the modal closes first, and the effect below
    // drops `pendingArm` as soon as `signOpen` turns false.
    const p = pendingArm;
    pendingArm = null;
    await loadTrades();
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
  const MODE_ICONS = { app: 'radio', background: 'moon', background_live: 'radio' };
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
    armed: 'histviz.alerts.state.watching',
    paused: 'histviz.alerts.filterPaused',
    fired: 'histviz.alerts.filterFired',
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
  /** A fire time as the list reads it: day, month and clock, no year, no seconds. */
  const short = (iso) =>
    new Date(iso).toLocaleString(undefined, { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' });
  const sourceOf = (a) => connectorNames[a.connector_id] ?? a.provider;
  /** Where an alert stands. A one-shot that fired is disabled by the engine, so a disabled
   *  alert that has fired reads as fired, not as paused by hand. */
  const stateOf = (a) => (isExpired(a) ? 'expired' : a.enabled ? 'watching' : a.last_fired_at ? 'fired' : 'paused');
  const STATES = ['watching', 'fired', 'paused', 'expired'];
  const armed = $derived(alerts.filter((a) => a.enabled).length);
  const matches = (a, f = filter) =>
    f === 'armed'
      ? stateOf(a) === 'watching'
      : f === 'error'
        ? !!a.last_error
        : f === 'all' || stateOf(a) === f;
  const counts = $derived(Object.fromEntries(Object.keys(FILTERS).map((f) => [f, alerts.filter((a) => matches(a, f)).length])));
  const shown = $derived(alerts.filter((a) => matches(a)));
  /** The list reads by state: what is watching first, then what fired, paused, expired. */
  const sections = $derived(
    STATES.map((st) => ({ st, rows: shown.filter((a) => stateOf(a) === st) })).filter((g) => g.rows.length)
  );

  // ── The form's foot: what the alert will do, in one line, and its two buttons ──
  const formShown = $derived(!!editing || (scope === 'chart' && !!coords));
  const canSubmit = $derived(
    !busy &&
      (atTime
        ? !!fireAt
        : !incomplete &&
          (forever || !!expiry) &&
          !(frequency === 'session' && !sessionId) &&
          !(frequency === 'cooldown' && !(Number(cooldown) > 0)))
  );
  const draftCond = $derived(
    conditionLabel({ lhs, rhs, op, fire_at: atTime && fireAt ? new Date(fireAt).toISOString() : null }, library, $t)
  );
  const summary = $derived.by(() => {
    if (!tradeOn || !tradeAccount) return $t('histviz.alerts.summary', { cond: draftCond });
    const account = tradeAccount.broker === 'paper' ? $t('histviz.alerts.trade.paper') : tradeAccount.name;
    if (draft.intent === 'exit') return $t('histviz.alerts.summaryExit', { account, cond: draftCond });
    const qty = draft.sizing === 'units' ? String(draft.qty ?? '') : `${draft.amount} ${$t(`histviz.alerts.trade.sizing.${draft.sizing}`)}`;
    return $t('histviz.alerts.summaryTrade', { side: $t(`histviz.trade.${draft.side}`), qty, account, cond: draftCond });
  });
  function cancelForm() {
    if (editing) stopEditing();
    else open = false;
  }
  const expiryOptions = $derived([
    { value: 'never', label: $t('histviz.alerts.expiryNever') },
    { value: 'date', label: $t('histviz.alerts.expiryOn') }
  ]);
  /** Every channel ticked again reads as "all": a channel added later is included too. */
  function pickChannel(id) {
    togglePick(id);
    if (Array.isArray(picked) && channels.length && channels.every((c) => picked.includes(c.id))) picked = 'all';
  }
</script>

{#snippet formFoot()}
  <p class="summary" title={summary}>{summary}</p>
  <button type="button" class="ghost" onclick={cancelForm}>{$t('common.cancel')}</button>
  <button class="primary" type="submit" form="alert-form" disabled={!canSubmit}>
    <Icon name={editing ? 'check' : 'bell'} size={14} />
    {$t(editing ? 'histviz.alerts.save' : 'histviz.alerts.create')}
  </button>
{/snippet}

<Modal bind:open title={$t('histviz.alerts.title')} size="lg" footer={formShown ? formFoot : undefined}>
  {#snippet head()}
    <h3 class="mtitle">{$t('histviz.alerts.title')}</h3>
    <div class="scopes" role="tablist">
      <button type="button" role="tab" class:on={scope === 'chart'} aria-selected={scope === 'chart'} disabled={!coords} onclick={() => setScope('chart')}>
        {coords ? `${coords.ticker} · ${coords.timeframe}` : $t('histviz.alerts.scopeChart')}
      </button>
      <button type="button" role="tab" class:on={scope === 'all'} aria-selected={scope === 'all'} onclick={() => setScope('all')}>
        {$t('histviz.alerts.scopeAll')}
        <span class="n">{scope === 'all' ? alerts.length : ''}</span>
      </button>
    </div>
  {/snippet}
  <ErrorText {error} />

  {#if formShown}
    {#if editing}
      <p class="editing">
        <Icon name="pencil" size={12} />
        {$t('histviz.alerts.editing', { name: editing.name || editing.ticker, ticker: `${editing.ticker} · ${editing.timeframe}` })}
        <button type="button" class="chip" onclick={stopEditing}>{$t('common.cancel')}</button>
      </p>
    {/if}

    <form
      id="alert-form"
      bind:this={formEl}
      class="new"
      onsubmit={(e) => {
        e.preventDefault();
        if (canSubmit) create();
      }}
    >
      <!-- `float` on every picker: the modal body scrolls its own overflow, and a scrolling
           ancestor clips an absolutely positioned menu down to a sliver. -->
      <section class="blk">
        <span class="sl">{$t('histviz.alerts.condition')}</span>
        <div class="cond">
          <span class="when">{$t('histviz.alerts.when')}</span>
          {#if atTime}
            <div class="side">
              <Dropdown bind:value={op} options={opOptions} ariaLabel={$t('histviz.alerts.op')} float />
            </div>
            <div class="side">
              <input type="datetime-local" step="1" bind:value={fireAt} aria-label={$t('histviz.alerts.fireAt')} />
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
        <p class="hint">{atTime ? $t('histviz.alerts.atTimeHint') : $t('histviz.alerts.rule')}</p>
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
      </section>

      {#if !atTime}
        <!-- Where it is watched: three cards, the live ones closed when the provider does not
             stream this instrument at this timeframe. -->
        <section class="blk">
          <span class="sl">{$t('histviz.alerts.mode')}</span>
          <div class="modes" role="radiogroup" aria-label={$t('histviz.alerts.mode')}>
            {#each MODES as m, i (m)}
              <button
                type="button"
                role="radio"
                class="mode"
                class:on={modeIdx === i}
                aria-checked={modeIdx === i}
                disabled={!liveOk && i !== 1}
                title={$t(`${MODE_KEYS[m]}Hint`)}
                onclick={() => (modeIdx = i)}
              >
                <Icon name={MODE_ICONS[m]} size={16} />
                <span class="mt">
                  <span class="mn">{$t(MODE_KEYS[m])}</span>
                  <span class="md">{$t(`${MODE_KEYS[m]}Short`)}</span>
                </span>
              </button>
            {/each}
          </div>
          {#if !liveOk}<p class="hint">{$t('histviz.alerts.noLive')}</p>{/if}
        </section>
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
                <button type="button" class="edit link" onclick={() => (sessionsOpen = true)}>{$t('histviz.alerts.sessionsLink')}</button>
              </span>
              <Dropdown bind:value={sessionId} options={sessionOptions} ariaLabel={$t('histviz.alerts.session')} float />
            </div>
          {/if}
          <div class="f">
            <span>{$t('histviz.alerts.expiry')}</span>
            <span class="expiry">
              <Dropdown
                bind:value={() => (forever ? 'never' : 'date'), (v) => (forever = v === 'never')}
                options={expiryOptions}
                ariaLabel={$t('histviz.alerts.expiry')}
                float
              />
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
      </div>

      <!-- Where it goes: the in-app notification always, then each granted channel. -->
      <section class="blk">
        <span class="sl">{$t('histviz.alerts.channels')}</span>
        <div class="chans">
          <span class="nchip on fixed" title={$t('histviz.alerts.channelsHint')}>
            <Icon name="bell" size={13} />
            {$t('histviz.alerts.inApp')}
          </span>
          {#each channels as c (c.id)}
            <button type="button" class="nchip" class:on={sendsTo(c.id)} aria-pressed={sendsTo(c.id)} onclick={() => pickChannel(c.id)}>
              <Icon name={sendsTo(c.id) ? 'check' : 'plus'} size={13} />
              {c.name}
            </button>
          {/each}
          <button type="button" class="nchip add-ch" onclick={() => (channelsOpen = true)}>
            <Icon name="plus" size={13} />
            {$t('histviz.alerts.addChannel')}
          </button>
        </div>
      </section>

      <!-- Trade on alert: one switch, the order ticket under it once on. -->
      <section class="trade" class:on={tradeOn} bind:this={tradeEl} aria-label={$t('histviz.alerts.trade.section')}>
        <div class="trade-h">
          <span class="tic"><Icon name="zap" size={16} /></span>
          <span class="tt">
            <span class="tn">
              {$t('histviz.alerts.trade.title')}
              {#if savedTrade}
                {@const st = statusOf(savedTrade)}
                <span class="tchip" class:on={st === 'armed'} class:warn={statusWarns(st)}>{$t(`histviz.alerts.trade.status.${st}`)}</span>
              {/if}
            </span>
            <span class="td">{$t('histviz.alerts.trade.subtitle')}</span>
          </span>
          <button
            type="button"
            class="sw"
            role="switch"
            aria-checked={tradeOn}
            aria-label={$t('histviz.alerts.trade.on')}
            title={$t('histviz.alerts.trade.riskNote')}
            disabled={!accounts.length}
            onclick={() => (tradeOn = !tradeOn)}
          ></button>
        </div>
        {#if !accounts.length}
          <p class="hint">{$t('histviz.alerts.trade.noAccounts')} <button type="button" class="link" onclick={() => (brokersOpen = true)}>{$t('histviz.alerts.trade.noAccountsAdd')}</button></p>
        {:else if tradeOn}
          <div class="trade-body">
            {#if noBackground}
              <div class="banner">
                <Icon name="alert-triangle" size={14} />
                <span>{$t('histviz.alerts.trade.background')}</span>
                <button type="button" class="bbtn" onclick={() => (signOpen = true)}>{$t('histviz.alerts.trade.sign')}</button>
              </div>
            {/if}
            <div class="tgrid two">
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

            {#if draft.intent === 'exit'}
              <p class="hint">{$t('histviz.alerts.trade.exitHint')}</p>
            {:else}
              <div class="tgrid two">
                <div class="tfld">
                  <span class="lab">{$t('histviz.trade.side')}</span>
                  <div class="seg sides" role="radiogroup" aria-label={$t('histviz.trade.side')}>
                    <button type="button" role="radio" aria-checked={draft.side === 'buy'} class="buy" class:on={draft.side === 'buy'} onclick={() => (draft.side = 'buy')}>
                      {$t('histviz.trade.buy')}
                    </button>
                    <button type="button" role="radio" aria-checked={draft.side === 'sell'} class="sell" class:on={draft.side === 'sell'} onclick={() => (draft.side = 'sell')}>
                      {$t('histviz.trade.sell')}
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
              </div>
            {/if}

            <div class="tfoot">
              {#if topts.reduceOnly && draft.intent !== 'exit'}
                <label class="chk" title={$t('histviz.trade.reduceOnlyHint')}>
                  <input type="checkbox" bind:checked={draft.reduceOnly} />
                  {$t('histviz.trade.reduceOnly')}
                </label>
              {/if}
              <span class="fb" title={$t('histviz.alerts.trade.fallbackHint')}>
                <span class="lab">{$t('histviz.alerts.trade.fallbackLabel')}</span>
                <Dropdown bind:value={draft.fallback} options={fallbackOptions} ariaLabel={$t('histviz.alerts.trade.fallbackLabel')} float />
              </span>
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

  {#if scope === 'chart'}
    <div class="gh lead"><span>{$t('histviz.alerts.onInstrument')}</span><span class="gl"></span></div>
  {:else}
  <div class="head">
    <p class="sum">
      <span>{$t('histviz.alerts.watchingN', { n: armed })}</span>
      <span class="of">{$t('histviz.alerts.ofN', { n: alerts.length })}</span>
    </p>
    <div class="filters" role="tablist">
      {#each Object.entries(FILTERS) as [f, k] (f)}
        <button type="button" role="tab" class="ftab" class:on={filter === f} aria-selected={filter === f} onclick={() => (filter = f)}>
          {$t(k)}
          {#if f !== 'all' && counts[f]}<span class="n">{counts[f]}</span>{/if}
        </button>
      {/each}
    </div>
    {#if scope === 'all' && armed > 0}
      <button type="button" class="pause-all" onclick={pauseAll}>
        <Icon name="pause" size={13} /> {$t('histviz.alerts.pauseAll')}
      </button>
    {/if}
  </div>
  {/if}

  {#snippet item(a)}
    {@const st = stateOf(a)}
    {@const tr = trades[a.id]}
    <li class:off={st !== 'watching'} class:cur-edit={editing?.id === a.id}>
      <span class="dot" class:on={st === 'watching'} class:err={!!a.last_error}></span>
      <span class="main">
        <span class="l1">
          <span class="nm">{a.name || a.ticker}</span>
          {#if a.op !== 'at_time'}
            <span class="badge" class:live={a.mode !== 'background'}>
              <Icon name={a.mode === 'background' ? 'moon' : 'radio'} size={11} />
              {$t(MODE_KEYS[a.mode] ?? MODE_KEYS.background)}
            </span>
          {/if}
          {#if tr}
            {@const ts = statusOf(tr)}
            <span class="tchip" class:on={ts === 'armed'} class:warn={statusWarns(ts)} title={lastText(tr)}>
              <Icon name="zap" size={11} />
              {$t('histviz.alerts.trade.badge')} {$t(`histviz.alerts.trade.status.${ts}`)}
            </span>
          {/if}
        </span>
        <span class="l2">
          <span class="tk">{a.ticker}</span>
          <span>· {a.timeframe} · {sourceOf(a)}</span>
          {#if a.op !== 'at_time' && a.frequency !== 'once'}<span>· {$t(FREQ_KEYS[a.frequency] ?? FREQ_KEYS.once)}</span>{/if}
          {#if st !== 'expired' && a.expires_at}<span>· {$t('histviz.alerts.expiresAt', { when: short(a.expires_at) })}</span>{/if}
        </span>
      </span>
      <span class="cond-text" title={label(a)}>{label(a)}</span>
      <!-- The state and the actions share one cell, swapped on hover: the row never reflows. -->
      <span class="end">
        <span class="face state" class:watching={st === 'watching' && !a.last_error}>
          {#if a.last_error}
            <span class="warn" title={a.last_error}><Icon name="alert-triangle" size={12} /> {$t('histviz.alerts.cannotRun')}</span>
          {:else if st === 'expired'}
            {$t('histviz.alerts.expired')}
          {:else if a.last_fired_at && st !== 'watching'}
            <span class="mono" title={when(a.last_fired_at)}>{short(a.last_fired_at)}</span>
          {:else if st === 'watching'}
            {$t('histviz.alerts.watching')}
          {:else}
            {$t('histviz.alerts.filterPaused')}
          {/if}
        </span>
        <span class="face acts">
          {#if scope === 'all'}
            <button class="icon" title={$t('histviz.alerts.openChart')} aria-label={$t('histviz.alerts.openChart')} onclick={() => openChart(a)}>
              <Icon name="candlestick" size={14} />
            </button>
          {/if}
          <button class="icon" class:on={editing?.id === a.id} title={$t('histviz.alerts.edit')} aria-label={$t('histviz.alerts.edit')} onclick={() => edit(a)}>
            <Icon name="pencil" size={14} />
          </button>
          {#if tr}
            {#if tr.armed_at}
              <button class="icon on" title={$t('histviz.alerts.trade.disarm')} aria-label={$t('histviz.alerts.trade.disarm')} onclick={() => disarmTrade(a)}>
                <Icon name="zap" size={14} />
              </button>
            {:else}
              <button class="icon" disabled={tradeBusy} title={$t('histviz.alerts.trade.rearm')} aria-label={$t('histviz.alerts.trade.rearm')} onclick={() => armTrade(a)}>
                <Icon name="zap" size={14} />
              </button>
            {/if}
          {/if}
          <button
            class="icon"
            disabled={st === 'expired' && !a.enabled}
            title={st === 'expired' && !a.enabled
              ? $t('histviz.alerts.expiredHint')
              : $t(a.enabled ? 'histviz.alerts.pause' : 'histviz.alerts.arm')}
            aria-label={$t(a.enabled ? 'histviz.alerts.pause' : 'histviz.alerts.arm')}
            onclick={() => toggle(a)}
          >
            <Icon name={a.enabled ? 'pause' : 'play'} size={14} />
          </button>
          <button class="icon danger" title={$t('common.remove')} aria-label={$t('common.remove')} onclick={() => remove(a)}>
            <Icon name="trash-2" size={14} />
          </button>
        </span>
      </span>
    </li>
  {/snippet}

  {#each sections as g (g.st)}
    {#if scope === 'all' || sections.length > 1}<div class="gh"><span>{$t(`histviz.alerts.state.${g.st}`)}</span><span class="n">{g.rows.length}</span><span class="gl"></span></div>{/if}
    <ul class="list">
      {#each g.rows as a (a.id)}{@render item(a)}{/each}
    </ul>
  {:else}
    {#if scope === 'all'}
      <p class="empty">{busy ? $t('common.loading') : $t('histviz.alerts.emptyAll')}</p>
    {:else}
      <p class="empty-box"><Icon name="info" size={14} /> {busy ? $t('common.loading') : $t('histviz.alerts.empty')}</p>
    {/if}
  {/each}
</Modal>

<BackgroundTradingModal bind:open={signOpen} account={signAccount} onsigned={onSigned} />
<ChannelsModal bind:open={channelsOpen} module="histviz" onchanged={loadChannels} />
<BrokersModal bind:open={brokersOpen} module="histviz" onchanged={loadTrades} />
<Modal bind:open={sessionsOpen} title={$t('settings.nav.sessions')} size="lg" onclose={loadSessions}>
  <MarketSessionsSection />
</Modal>

<style>
  .mtitle {
    margin: 0;
    color: var(--text);
    font-size: var(--text-md);
    font-weight: var(--fw-medium);
  }
  /* Scope: two keys in one well, the picked one raised. */
  .scopes {
    display: inline-flex;
    gap: 2px;
    padding: 3px;
    border-radius: var(--radius);
    background: var(--surface-2);
  }
  .scopes button {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    height: 28px;
    padding: 0 var(--space-3);
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--muted);
    font-size: var(--text-sm);
    white-space: nowrap;
    cursor: pointer;
  }
  .scopes button.on {
    background: var(--surface-raised, var(--surface));
    color: var(--text);
  }
  .scopes button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .n {
    color: var(--muted);
    font-family: var(--mono);
    font-size: var(--text-xs);
  }
  .n:empty {
    display: none;
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
    align-items: center;
    gap: var(--space-2) var(--space-4);
    margin: var(--space-3) 0 var(--space-2);
  }
  .sum {
    display: inline-flex;
    gap: var(--space-1);
    margin: 0;
    color: var(--text);
    font-size: var(--text-sm);
    white-space: nowrap;
  }
  .sum .of {
    color: var(--muted);
  }
  .filters {
    display: flex;
    flex-wrap: wrap;
    gap: 2px;
    flex: 1;
  }
  .ftab {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 30px;
    padding: 0 var(--space-3);
    border: 0;
    border-radius: var(--radius);
    background: none;
    color: var(--muted);
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .ftab:hover {
    color: var(--text);
  }
  .ftab.on {
    background: var(--surface-2);
    color: var(--text);
  }
  .pause-all {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    height: 30px;
    padding: 0 var(--space-3);
    border: 0;
    border-radius: var(--radius);
    background: var(--surface-2);
    color: var(--text);
    font-size: var(--text-sm);
    white-space: nowrap;
    cursor: pointer;
  }
  .pause-all :global(svg) {
    color: var(--muted);
  }
  .pause-all:hover {
    background: color-mix(in srgb, var(--text) 8%, var(--surface-2));
  }
  .gh {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: var(--space-3) 0 var(--space-1);
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .gl {
    flex: 1;
    height: var(--hairline);
    background: var(--border);
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
  .empty {
    margin: var(--space-6) 0;
    color: var(--muted);
    font-size: var(--text-sm);
    text-align: center;
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
  }
  .new {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    padding: var(--space-1) 0 var(--space-4);
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
    align-items: flex-end;
  }
  .f {
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: var(--text-sm);
    color: var(--muted);
    min-width: 120px;
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
  .edit {
    margin-left: var(--space-1);
    font-size: 0.68rem;
  }
  .link {
    background: none;
    border: none;
    padding: 0;
    color: var(--accent);
    font: inherit;
    cursor: pointer;
  }
  .edit.link {
    font-size: 0.68rem;
  }
  .link:hover {
    text-decoration: underline;
  }
  .badge {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 20px;
    padding: 0 var(--space-2);
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    color: var(--muted);
    font-size: var(--text-xs);
    white-space: nowrap;
  }
  .badge.live :global(svg) {
    color: var(--accent);
  }
  .hint {
    font-size: 0.72rem;
    color: var(--muted);
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .list li {
    display: grid;
    grid-template-columns: 8px minmax(0, 1.3fr) minmax(0, 1fr) auto;
    align-items: center;
    gap: var(--space-4);
    min-height: 52px;
    padding: 6px var(--space-3);
    border-radius: var(--radius-lg);
  }
  .list li:hover,
  .list li:focus-within,
  .list li.cur-edit {
    background: var(--surface-2);
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--faint, var(--border));
  }
  .dot.on {
    background: var(--green);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--green) 20%, transparent);
  }
  .dot.err {
    background: var(--amber);
    box-shadow: none;
  }
  .main {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .l1 {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }
  .nm {
    overflow: hidden;
    color: var(--text);
    font-size: var(--text-base);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .list li.off .nm {
    color: color-mix(in srgb, var(--text) 80%, var(--muted));
  }
  .l2 {
    display: flex;
    gap: 4px;
    min-width: 0;
    overflow: hidden;
    color: var(--muted);
    font-size: var(--text-sm);
    white-space: nowrap;
  }
  .tk {
    font-family: var(--mono);
  }
  .cond-text {
    overflow: hidden;
    color: var(--muted);
    font-family: var(--mono);
    font-size: var(--text-sm);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .end {
    display: inline-grid;
    justify-items: end;
  }
  .face {
    display: inline-flex;
    align-items: center;
    justify-content: flex-end;
    gap: 2px;
    grid-area: 1 / 1;
  }
  .state {
    color: var(--muted);
    font-size: var(--text-sm);
    white-space: nowrap;
  }
  .state.watching {
    color: var(--green);
  }
  .mono {
    font-family: var(--mono);
  }
  .acts {
    visibility: hidden;
  }
  .list li:hover .acts,
  .list li:focus-within .acts,
  .list li.cur-edit .acts {
    visibility: visible;
  }
  .list li:hover .state,
  .list li:focus-within .state,
  .list li.cur-edit .state {
    visibility: hidden;
  }
  .warn {
    color: var(--amber);
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .icon {
    display: inline-grid;
    place-items: center;
    width: 30px;
    height: 30px;
    padding: 0;
    background: none;
    border: 0;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
  }
  .icon:hover:not(:disabled) {
    color: var(--text);
    background: color-mix(in srgb, var(--text) 8%, var(--surface-2));
  }
  .icon:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .icon.danger:hover:not(:disabled) {
    color: var(--red);
  }

  .tgrid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: var(--space-3);
    align-items: end;
  }
  .tgrid.two {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
  .tfld {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
  }
  .tfld .lab {
    color: var(--muted);
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
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 20px;
    padding: 0 var(--space-2);
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    color: var(--muted);
    font-size: var(--text-xs);
    white-space: nowrap;
  }
  .tchip.on {
    background: color-mix(in srgb, var(--amber) 14%, transparent);
    color: var(--amber);
  }
  .tchip.warn {
    background: color-mix(in srgb, var(--red) 14%, transparent);
    color: var(--red);
  }
  /* ── The form: titled blocks, one frame per control ── */
  /* Focus as on the chart, wherever the modal opens (a watchlist too): a quiet hairline,
     no halo around the field. */
  .new :global(:is(input:not([type='checkbox'], [type='radio']), select, textarea):focus) {
    border-color: color-mix(in srgb, var(--accent) 40%, var(--border));
    box-shadow: none;
    outline: none;
  }
  /* The level is the number read most: a wider, larger field than the operand default. */
  .cond .side :global(input.val) {
    width: 140px;
    max-width: none;
    font-family: var(--mono);
    font-size: var(--text-base);
  }
  .blk {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .sl {
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .blk > .hint {
    margin: 0;
  }
  .cond {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }
  .when {
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .modes {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: var(--space-2);
  }
  .mode {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    padding: var(--space-3);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius-lg);
    background: none;
    color: var(--muted);
    text-align: left;
    cursor: pointer;
  }
  .mode:hover:not(:disabled) {
    background: var(--surface-2);
  }
  .mode.on {
    border-color: color-mix(in srgb, var(--accent) 55%, var(--border));
    background: color-mix(in srgb, var(--accent) 7%, transparent);
    color: var(--accent);
  }
  .mode:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .mt {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .mn {
    color: var(--text);
    font-size: var(--text-sm);
  }
  .md {
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .chans {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .nchip {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    height: 30px;
    padding: 0 var(--space-3);
    border: 0;
    border-radius: var(--radius);
    background: var(--surface-2);
    color: var(--muted);
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .nchip.on {
    background: color-mix(in srgb, var(--accent) 12%, var(--surface-2));
    color: var(--text);
  }
  .nchip.on :global(svg) {
    color: var(--accent);
  }
  .nchip.fixed {
    cursor: default;
  }
  .nchip.add-ch:hover {
    color: var(--text);
  }

  /* ── Trade on alert: a card, its switch in the corner, the ticket under it ── */
  .trade {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius-lg);
  }
  .trade.on {
    padding-bottom: var(--space-4);
  }
  .trade > .hint {
    margin: 0;
  }
  .trade-h {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }
  .tic {
    display: inline-grid;
    place-items: center;
    flex: none;
    width: 34px;
    height: 34px;
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--amber) 14%, transparent);
    color: var(--amber);
  }
  .tt {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
    min-width: 0;
  }
  .tn {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--text);
    font-size: var(--text-base);
  }
  .td {
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .sw {
    position: relative;
    flex: none;
    width: 36px;
    height: 20px;
    padding: 0;
    border: none;
    border-radius: 10px;
    background: var(--border-control);
    cursor: pointer;
    transition: background var(--dur-fast) var(--ease);
  }
  .sw::after {
    content: '';
    position: absolute;
    top: 3px;
    left: 3px;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: #fff;
    transition: transform var(--dur-fast) var(--ease);
  }
  .sw[aria-checked='true'] {
    background: var(--accent);
  }
  .sw[aria-checked='true']::after {
    transform: translateX(16px);
  }
  .sw:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .trade-body {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    font-size: var(--text-sm);
  }
  .trade-body .hint {
    margin: 0;
  }
  .banner {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-2) var(--space-2) var(--space-3);
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--amber) 10%, transparent);
    color: var(--amber);
  }
  .banner > span {
    flex: 1;
    min-width: 0;
  }
  .bbtn {
    flex: none;
    height: 28px;
    padding: 0 var(--space-3);
    border: 0;
    border-radius: var(--radius-sm);
    background: color-mix(in srgb, var(--amber) 18%, transparent);
    color: var(--amber);
    font-size: var(--text-sm);
    white-space: nowrap;
    cursor: pointer;
  }
  .bbtn:hover {
    background: color-mix(in srgb, var(--amber) 26%, transparent);
  }
  .tfld .lab,
  .fb .lab {
    color: var(--muted);
  }
  /* A row of choices in one well: the picked one is raised. */
  .seg {
    display: flex;
    gap: 2px;
    height: var(--control-h);
    padding: 2px;
    border-radius: var(--radius);
    background: var(--surface-2);
  }
  .seg button {
    flex: 1;
    min-width: 0;
    padding: 0 var(--space-1);
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--muted);
    font-size: var(--text-sm);
    cursor: pointer;
    white-space: nowrap;
  }
  .seg button:hover {
    color: var(--text);
  }
  .seg button.on {
    background: var(--surface-raised, var(--surface));
    color: var(--text);
  }
  .sides button.buy.on {
    background: color-mix(in srgb, var(--green) 16%, var(--surface));
    color: var(--green);
  }
  .sides button.sell.on {
    background: color-mix(in srgb, var(--red) 16%, var(--surface));
    color: var(--red);
  }
  .tfoot {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-4);
    padding-top: var(--space-3);
    border-top: var(--hairline) solid var(--border);
  }
  .fb {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
  }
  .empty-box {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    margin: 0;
    padding: var(--space-3) var(--space-4);
    border-radius: var(--radius-lg);
    background: var(--surface-2);
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .gh.lead {
    margin-top: 0;
  }

  /* ── Foot: the sentence the alert will act on, then Cancel and the main button ── */
  .summary {
    flex: 1;
    min-width: 0;
    margin: 0 auto 0 0;
    overflow: hidden;
    color: var(--muted);
    font-size: var(--text-sm);
    text-overflow: ellipsis;
    white-space: nowrap;
    align-self: center;
  }
  .ghost {
    height: 34px;
    padding: 0 var(--space-3);
    border: 0;
    border-radius: var(--radius);
    background: none;
    color: var(--text);
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .ghost:hover {
    background: var(--surface-2);
  }
  .primary {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    height: 34px;
    padding: 0 var(--space-4);
    border: 0;
    border-radius: var(--radius);
    background: var(--accent);
    color: var(--accent-contrast);
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    white-space: nowrap;
    cursor: pointer;
  }
  .primary:hover:not(:disabled) {
    background: var(--accent-hover);
  }
  .primary:disabled {
    opacity: 0.45;
    cursor: default;
  }
  /* Phone: the condition label sits above its fields, the form fields take the line, list
     entries wrap and every control takes a finger. */
  @media (max-width: 767px) {
    .scopes button, .ftab, .pause-all, .chip, .nchip, .seg button, .ghost, .primary, .bbtn { min-height: 44px; }
    .cond .when { display: none; }
    .modes { grid-template-columns: minmax(0, 1fr); }
    .row > .f, .f.wide { flex: 1 1 100%; min-width: 0; }
    .add, .seg, .expiry { min-height: 44px; height: auto; }
    .chk { min-height: 44px; }
    .tgrid, .tgrid.two { grid-template-columns: repeat(2, minmax(0, 1fr)); }
    .summary { flex: 1 1 100%; }
    /* No hover on a phone: the condition goes under the name, state and actions both show. */
    .list li { grid-template-columns: 8px minmax(0, 1fr); row-gap: var(--space-1); }
    .cond-text, .end { grid-column: 2; }
    .end { display: flex; flex-wrap: wrap; justify-content: space-between; align-items: center; }
    .acts { visibility: visible; }
    .list li .state { visibility: visible; white-space: normal; }
    .l1 { flex-wrap: wrap; }
    .icon { min-width: 44px; min-height: 44px; }
  }
</style>
