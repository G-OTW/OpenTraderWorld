<script>
  // Order ticket: one for the whole workspace, like the quick-backtest strip. It always works
  // on the active chart, so picking a chart and trading it are the same act.
  //
  // It takes its place in the layout and the charts make room (see the page): along an edge,
  // or slipped between two rows or two columns of charts. Placed from its own header;
  // collapsed it is that header alone, or a thin strip when it stands on a side.
  //
  // Five tabs: the ticket, then the trading accounts' positions, orders, money and order log
  // (BrokerTabs), filtered to one broker or all. The ticket reads top to bottom on a side
  // (Sell/Buy at the market, the type, its prices, the size, the exits, the extra settings,
  // the journal, the button) and lays the same sections out side by side along the top or the
  // bottom. Every price can be typed as a price or as a distance (ticks when the instrument's
  // tick size is known, points otherwise) from what it is measured against: the side an entry
  // would cross, the entry for its exits. The button sends the order to the chart's account
  // (the page does, see `addOrders`), and the orders tab follows it there.
  import { onMount, untrack } from 'svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import { clickOutside } from '$lib/ui/clickOutside.js';
  import { t } from '$lib/i18n';
  import { journalApi } from '$lib/modules/journal/api.js';
  import { histvizApi } from './api.js';
  import {
    typesFor,
    tifsFor,
    defaultTif,
    optionsFor,
    exitTifsFor,
    triggeredType,
    sizeFrom,
    SIZINGS,
    TICKET_DOCKS,
    dockIsRow,
    pricesOf,
    buildOrders,
    roundPrice
  } from './orders.js';
  import { makePriceFmt, priceDigitsOfValues } from './format.js';
  import BrokerTabs from './BrokerTabs.svelte';
  import SizeField from './SizeField.svelte';

  let {
    target = null, // { symbol, instrument, last, digits, bid, ask, tick, pointValue, qtyStep, leverage, marginMode, kind } for the active chart, or null
    account = null, // the active chart's account, or null when it cannot be traded
    lock = '', // why not, as the page words it
    orders = [],
    dock = 'bottom', // where it sits now (the page falls back from a middle it cannot use)
    docks = TICKET_DOCKS, // the positions the current grid allows
    onpickdock,
    expanded = $bindable(true),
    tab = $bindable('ticket'),
    /** dock -> the size the user dragged it to there (a height along a row, a width on a
     *  side), kept by the page. A dock with none takes the default. */
    sizes = $bindable({}),
    /** A request to fill the form, from the `+` on a chart: { side, type, price, n }, or to
     *  protect a position: { protect: { symbol, side, qty }, n } (an OCO of a take profit and
     *  a stop loss on the other side). `n` changes on every request so the same price asked
     *  twice is applied twice. */
    prefill = null,
    oncreate,
    oncancel,
    oncancelall,
    /** (accountId, orderId): a broker order this page did not place (see BrokerTabs). */
    oncancelbroker,
    /** (accountId, clientIds): orders the broker never received, sent again or dropped. */
    onresendunsent,
    ondismissunsent,
    /** (localId) / (accountId, orderId): open an order in the ticket to change it. */
    oneditline,
    oneditbroker,
    /** (accountId, position): close it at the market (the page asks first). */
    oncloseposition,
    /** (accountId, position): open the ticket on its chart with exits to protect it. */
    onprotectposition,
    /** The trading accounts, their last read books, and the panel's broker filter. */
    tradeAccounts = [],
    accountBooks = {},
    /** account|symbol -> a chart's live price, to mark positions with (see BrokerTabs). */
    livePrices = {},
    /** account|order id -> where a trailing stop trails now, as a chart works it out. */
    trailPrices = {},
    /** accountId -> { native_id: live mark }, for the positions no chart shows. */
    accountMarks = {},
    broker = $bindable(''),
    /** A working order to change instead of a new one: { n, account, ref, symbol, side, type,
     *  qty, limit, stop, tif, expires }. `n` changes on every request. Side and type stay as
     *  they are; prices, size and time in force are sent as a change (`onamend`). */
    edit = null,
    onamend,
    onendedit,
    /** Halts in force on the chart's account (or on every account): `[{ id, account_id,
     *  reason, until }]`, each with a Lift (`onlifthalt(id)`, the page asks the password). */
    halts = [],
    onlifthalt,
    /** The global kill switch: the page confirms, then cancels and halts every account. */
    onkillswitch,
    killing = false,
    onclose
  } = $props();

  const haltWhen = (iso) => new Date(iso).toLocaleString([], { dateStyle: 'short', timeStyle: 'short' });
  /** One halt as the banner says it: what stopped trading, where, and until when. */
  function haltText(h) {
    const scope = h.account_id == null ? $t('histviz.trade.halt.every') : (tradeAccounts.find((a) => a.id === h.account_id)?.name ?? '');
    const until = h.until ? $t('brokers.risk.until', { time: haltWhen(h.until) }) : $t('brokers.risk.untilLifted');
    const reason = ['loss_limit', 'position_drift'].includes(h.reason) ? h.reason : 'kill_switch';
    return $t(`histviz.trade.halt.${reason}`, { scope, until });
  }

  let side = $state('buy');
  let type = $state('limit');
  let qty = $state(1);
  let px = $state({ limit: '', stop: '', tp: '', sl: '' });
  /** Which price fields are typed as a distance instead of a price. */
  let byOffset = $state({ limit: false, stop: false, tp: false, sl: false });
  let tif = $state('day');
  /** What the account's broker takes; a choice it does not take falls back to its default. */
  const tifs = $derived(tifsFor(account));
  const tif0 = $derived(defaultTif(tifs));
  $effect(() => {
    if (!tifs.includes(tif)) tif = tif0;
  });
  let expDate = $state('');
  let expTime = $state('');
  /** What the account's broker takes beyond the plain order: each control shows only then. */
  const types = $derived(typesFor(account));
  $effect(() => {
    if (!edit && !types.includes(type)) type = types.includes('limit') ? 'limit' : (types[0] ?? 'market');
  });
  const opts = $derived(optionsFor(account));
  let reduceOnly = $state(false);
  const canReduce = $derived(opts.reduceOnly);
  let postOnly = $state(false);
  let trailKind = $state('percent');
  let trailValue = $state('1');
  $effect(() => {
    if (opts.trail.length && !opts.trail.includes(trailKind)) trailKind = opts.trail[0];
  });
  /** A trailing stop that waits for a price before it trails (the stop field holds it). */
  let trailStartOn = $state(false);
  let display = $state('');
  let outsideRth = $state(false);
  let trigger = $state('');
  let closePos = $state(false);
  let adaptive = $state('');
  let exitTif = $state('gtc');
  const exitTifs = $derived(exitTifsFor(tifs));
  $effect(() => {
    if (exitTifs.length && !exitTifs.includes(exitTif)) exitTif = exitTifs[0];
  });
  /** How the size is given: units, an amount of money, or the money at risk to the stop loss. */
  let sizing = $state('units');
  let amount = $state('');
  /** A position being protected: the ticket's note, until the order goes or the form changes. */
  let protecting = $state(null);
  let tpOn = $state(false);
  let slOn = $state(false);
  let exitsOpen = $state(true);
  let extraOpen = $state(true);
  let error = $state('');

  const row = $derived(dockIsRow(dock));
  /** Standing on a side (or between columns): a column, and a strip when collapsed. */
  const upright = $derived(!row);

  // ── Resize ──
  // The edge facing the charts is a grip: a height along a row, a width on a side, kept per
  // dock so the bottom and the right each remember their own.
  const GRIP_EDGE = { bottom: 'top', top: 'bottom', 'center-h': 'bottom', left: 'right', right: 'left', 'center-v': 'right' };
  const gripEdge = $derived(GRIP_EDGE[dock] ?? 'top');
  let el = $state(null);
  let grip = $state(null);
  let drag = null;
  /** The size while a drag is in progress, written to `sizes` (and stored) on release. */
  let live = $state(null);
  const size = $derived(live ?? sizes?.[dock] ?? null);

  // Rows leave the charts at least half the height (60% around a middle ticket); a side
  // keeps the form readable and the charts most of the width.
  function clampSize(v) {
    const vh = globalThis.innerHeight ?? 900;
    const vw = globalThis.innerWidth ?? 1400;
    const [lo, hi] = row ? [120, vh * (dock === 'center-h' ? 0.4 : 0.5)] : [240, Math.min(600, vw * 0.4)];
    return Math.round(Math.max(lo, Math.min(v, hi)));
  }
  /** Dragging towards the charts grows the ticket: up from the bottom, left from the right. */
  const growSign = $derived(gripEdge === 'top' || gripEdge === 'left' ? -1 : 1);

  function onGripDown(e) {
    if (e.button !== 0 || !el) return;
    const box = el.getBoundingClientRect();
    drag = { p0: row ? e.clientY : e.clientX, s0: row ? box.height : box.width };
    grip?.setPointerCapture?.(e.pointerId);
    e.preventDefault();
  }
  function onGripMove(e) {
    if (!drag) return;
    live = clampSize(drag.s0 + growSign * ((row ? e.clientY : e.clientX) - drag.p0));
  }
  function onGripUp(e) {
    if (!drag) return;
    drag = null;
    grip?.releasePointerCapture?.(e.pointerId);
    if (live != null) sizes = { ...sizes, [dock]: live };
    live = null;
  }
  function onGripKey(e) {
    const axis = row ? ['ArrowUp', 'ArrowDown'] : ['ArrowLeft', 'ArrowRight'];
    const i = axis.indexOf(e.key);
    if (i < 0 || !el) return;
    const box = el.getBoundingClientRect();
    const step = (e.shiftKey ? 60 : 20) * (i ? 1 : -1) * growSign;
    sizes = { ...sizes, [dock]: clampSize((row ? box.height : box.width) + step) };
    e.preventDefault();
  }

  const need = $derived(pricesOf(type));
  /** The stop field: a trigger, a trailing stop's optional start, or a plain stop. */
  const showStop = $derived(need.stop || (type === 'trailing_stop' && opts.trailStart));
  const stopLabel = $derived(
    type === 'mit' || type === 'lit' ? 'histviz.trade.triggerPrice' : type === 'trailing_stop' ? 'histviz.trade.trailStart' : 'histviz.trade.stopPrice'
  );
  const digits = $derived(target?.digits ?? 2);
  const fmt = $derived(makePriceFmt(digits));
  const step = $derived(digits ? 10 ** -digits : 'any');
  /** One step of a distance: the instrument's tick when its facts give one, else one point. */
  const tick = $derived(Number(target?.tick) > 0 ? Number(target.tick) : null);

  const pos = (v) => (Number(v) > 0 ? Number(v) : null);
  const hasQuote = $derived(!!pos(target?.bid) && !!pos(target?.ask));
  const bidPx = $derived(pos(target?.bid) ?? pos(target?.last));
  const askPx = $derived(pos(target?.ask) ?? pos(target?.last));
  const spread = $derived(hasQuote ? target.ask - target.bid : null);
  /** What an entry is measured from: the side it would cross, or the last close. */
  const refName = $derived(hasQuote ? (side === 'buy' ? 'ask' : 'bid') : 'last');
  const entryRef = $derived(side === 'buy' ? askPx : bidPx);
  /** Where the entry would fill: what its exits are measured from. */
  const entryPx = $derived(
    type === 'limit' || type === 'stop_limit' || type === 'lit' ? pos(px.limit) : type === 'stop' || type === 'mit' ? pos(px.stop) : entryRef
  );

  /** Contracts and shares trade whole; the rest in fractions. */
  const whole = $derived(['future', 'stock', 'etf', 'option', 'fund'].includes(String(target?.kind ?? '').toLowerCase()));
  /** The size the order goes out with: typed in units, or worked out from the amount. */
  const orderQty = $derived(
    sizing === 'units'
      ? Number(qty)
      : sizeFrom(sizing, amount, { entry: entryPx, stop: slOn ? pos(px.sl) : null, pointValue: target?.pointValue, whole, step: target?.qtyStep })
  );

  const refOf = (k) => (k === 'tp' || k === 'sl' ? entryPx : entryRef);
  /** An entry's distance is signed; an exit's counts towards its own side of the entry. */
  const dirOf = (k) => (k === 'tp' ? (side === 'buy' ? 1 : -1) : k === 'sl' ? (side === 'buy' ? -1 : 1) : 1);

  function offsetOf(k) {
    const p = pos(px[k]);
    const ref = refOf(k);
    if (!p || !ref) return '';
    const d = ((p - ref) * dirOf(k)) / (tick ?? 1);
    return tick ? Math.round(d) : roundPrice(d, digits);
  }
  function setOffset(k, v) {
    const n = Number(v);
    const ref = refOf(k);
    if (v === '' || v == null || !Number.isFinite(n) || !ref) return;
    px[k] = String(roundPrice(ref + n * dirOf(k) * (tick ?? 1), digits));
  }
  const distText = (n) => (tick ? $t('histviz.trade.ticks', { n }) : $t('histviz.trade.points', { n: fmt(n) }));
  /** The distance as words, beside a field typed as a price. */
  function sideText(k) {
    const o = offsetOf(k);
    if (o === '') return '';
    if (k === 'tp' || k === 'sl') return distText(o);
    const name = $t(`histviz.trade.ref.${refName}`);
    return o ? `${name} ${o > 0 ? '+' : '−'} ${distText(Math.abs(o))}` : name;
  }

  /** Turning an exit on with no price puts it at a modest distance from the entry. */
  function toggleExit(k) {
    const on = k === 'tp' ? (tpOn = !tpOn) : (slOn = !slOn);
    if (!on || pos(px[k]) || !entryPx) return;
    const d = entryPx * (k === 'tp' ? 0.005 : 0.0025);
    const snapped = tick ? Math.max(1, Math.round(d / tick)) * tick : d;
    px[k] = String(roundPrice(entryPx + snapped * dirOf(k), digits));
  }

  // A new chart starts its prices at that chart's market, not at the last one's.
  let seenInstrument = '';
  $effect(() => {
    const key = target?.instrument ?? '';
    if (key === seenInstrument) return;
    seenInstrument = key;
    // An order being changed keeps its own prices whatever chart turns active.
    if (untrack(() => edit)) return;
    const last = pos(target?.last);
    const p = last ? String(roundPrice(last, target.digits ?? 2)) : '';
    px = { limit: p, stop: p, tp: '', sl: '' };
    error = '';
  });

  // ── Changing a working order ──
  // The form takes the order's values; what differs from them on submit is the change sent.
  let seenEdit = 0;
  let orig = null;
  $effect(() => {
    const e = edit;
    if (!e || e.n === seenEdit) return;
    seenEdit = e.n;
    side = e.side;
    type = e.type;
    qty = Number(e.qty);
    px = { limit: e.limit > 0 ? String(e.limit) : '', stop: e.stop > 0 ? String(e.stop) : '', tp: '', sl: '' };
    byOffset = { limit: false, stop: false, tp: false, sl: false };
    tpOn = false;
    slOn = false;
    sizing = 'units';
    protecting = null;
    tif = e.tif ?? tif0;
    if (e.tif === 'gtd' && e.expires) {
      const d = new Date(e.expires);
      const pad = (n) => String(n).padStart(2, '0');
      expDate = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
      expTime = `${pad(d.getHours())}:${pad(d.getMinutes())}`;
    }
    orig = { qty: Number(e.qty), limit: Number(e.limit) || null, stop: Number(e.stop) || null, tif: e.tif ?? null };
    tab = 'ticket';
    expanded = true;
    error = '';
  });

  /** What the form changes in the order being edited, or null when nothing. */
  function changes() {
    const out = {};
    const n = (v) => (Number(v) > 0 ? Number(v) : null);
    if (n(qty) !== orig.qty) out.qty = n(qty);
    if (need.limit && n(px.limit) !== orig.limit) out.limit = n(px.limit);
    if (need.stop && n(px.stop) !== orig.stop) out.stop = n(px.stop);
    // Time in force goes only when touched: a broker order read from the account says none.
    if (tif !== (orig.tif ?? tif0) || (tif === 'gtd' && expDate)) {
      out.tif = tif;
      if (tif === 'gtd') out.expires = expDate ? new Date(`${expDate}T${expTime || '00:00'}`).getTime() : null;
    }
    return Object.keys(out).length ? out : null;
  }

  function amend() {
    error = '';
    const c = changes();
    if (!c) {
      error = $t('histviz.trade.err.nothingChanged');
      return;
    }
    if (Object.entries(c).some(([k, v]) => k !== 'tif' && k !== 'expires' && !(v > 0))) {
      error = $t('histviz.trade.err.qty');
      return;
    }
    if (c.tif === 'gtd' && !(c.expires > Date.now())) {
      error = $t('histviz.trade.err.expires');
      return;
    }
    onamend?.(edit, c);
  }

  let seenPrefill = 0;
  $effect(() => {
    const p = prefill;
    if (!p || p.n === seenPrefill) return;
    seenPrefill = p.n;
    if (p.protect) {
      // An OCO on the other side for what is held: a take profit and a stop loss around the
      // market, one cancelling the other, reduce-only where the broker has it.
      const pr = p.protect;
      side = pr.side === 'short' ? 'buy' : 'sell';
      type = types.includes('oco') ? 'oco' : 'stop';
      sizing = 'units';
      qty = Number(pr.qty) || qty;
      reduceOnly = canReduce;
      const ref = pos(target?.last);
      if (ref) {
        const away = (f) => String(roundPrice(ref * (1 + f * (side === 'sell' ? 1 : -1)), digits));
        px.limit = away(0.005);
        px.stop = away(-0.0025);
      }
      protecting = pr;
    } else {
      side = p.side ?? side;
      type = p.type ?? type;
      protecting = null;
      if (p.price > 0) {
        if (pricesOf(type).limit) px.limit = String(p.price);
        if (pricesOf(type).stop) px.stop = String(p.price);
      }
    }
    tab = 'ticket';
    expanded = true;
    error = '';
  });

  // A good-till-date order starts at this time tomorrow.
  $effect(() => {
    if (tif !== 'gtd' || expDate) return;
    const d = new Date(Date.now() + 86_400_000);
    const pad = (n) => String(n).padStart(2, '0');
    expDate = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
    expTime = `${pad(d.getHours())}:${pad(d.getMinutes())}`;
  });

  // ── Journal ──
  // The journal's categories are its books. Null while unknown, or when the journal cannot be
  // reached: the row is then left out rather than offering a book nothing can be booked into.
  let journals = $state(null);
  onMount(() => {
    journalApi
      .listCategories()
      .then((list) => (journals = list ?? []))
      .catch(() => (journals = null));
  });
  // The pick is the chart instrument's, kept by the server: every position opened on it while
  // a journal is set is booked there, whoever closes it (`journal_import::chart`). Another
  // pick books the next positions elsewhere; one open now finishes where it opened.
  let journal = $state('');
  const linked = $derived.by(() => {
    const [, asset_type = '', ...rest] = String(target?.instrument ?? '').split('|');
    const ticker = rest.join('|');
    return ticker && asset_type ? { ticker, asset_type } : null;
  });
  const linkedKey = $derived(linked ? `${linked.asset_type}|${linked.ticker}` : '');
  $effect(() => {
    const key = linkedKey;
    const inst = untrack(() => linked);
    journal = '';
    if (!inst) return;
    histvizApi
      .journalLink(inst)
      .then((id) => {
        if (key === linkedKey) journal = id ?? '';
      })
      .catch(() => {});
  });
  async function pickJournal(id) {
    const inst = linked;
    if (!inst) return;
    const before = journal;
    journal = id;
    try {
      journal = (await histvizApi.setJournalLink(inst, id || null)) ?? '';
    } catch (e) {
      journal = before;
      error = e?.message ?? String(e);
    }
  }
  const journalOptions = $derived([
    { value: '', label: $t('histviz.trade.noJournal') },
    ...(journals ?? []).map((c) => ({ value: c.id, label: c.name, color: c.color ?? undefined }))
  ]);
  let newOpen = $state(false);
  let newName = $state('');
  let newErr = $state('');
  let newBusy = $state(false);
  function openNewJournal() {
    newName = '';
    newErr = '';
    newOpen = true;
  }
  async function createJournal() {
    const name = newName.trim();
    if (!name) {
      newErr = $t('histviz.trade.journalNameRequired');
      return;
    }
    newBusy = true;
    newErr = '';
    try {
      const cat = await journalApi.addCategory(name);
      journals = [...(journals ?? []), cat];
      await pickJournal(cat.id);
      newOpen = false;
    } catch (e) {
      newErr = e?.message ?? String(e);
    } finally {
      newBusy = false;
    }
  }

  /** The band each position fills in its icon, in a 16 x 16 box framed at (1.5, 2.5, 13, 11). */
  const DOCK_BAND = {
    bottom: { x: 1.5, y: 9.5, w: 13, h: 4 },
    top: { x: 1.5, y: 2.5, w: 13, h: 4 },
    left: { x: 1.5, y: 2.5, w: 4.5, h: 11 },
    right: { x: 10, y: 2.5, w: 4.5, h: 11 },
    'center-h': { x: 1.5, y: 6.5, w: 13, h: 3 },
    'center-v': { x: 6.5, y: 2.5, w: 3, h: 11 }
  };
  // The ticket clips its overflow, so the menu is fixed to the viewport, measured from its
  // button when it opens, and opens upwards when the ticket sits low on the screen.
  let dockOpen = $state(false);
  let dockBtn = $state(null);
  let dockAt = $state(null);
  function toggleDock() {
    if (dockOpen || !dockBtn) {
      dockOpen = false;
      return;
    }
    const r = dockBtn.getBoundingClientRect();
    const right = Math.max(4, window.innerWidth - r.right);
    dockAt = r.bottom + 220 > window.innerHeight
      ? { right, bottom: window.innerHeight - r.top + 4 }
      : { right, top: r.bottom + 4 };
    dockOpen = true;
  }
  function pickDock(d) {
    dockOpen = false;
    if (dock !== d) onpickdock?.(d);
  }
  const tifOptions = $derived(tifs.map((v) => ({ value: v, label: $t(`histviz.trade.tif.${v}`) })));
  const exitTifOptions = $derived(exitTifs.map((v) => ({ value: v, label: $t(`histviz.trade.tif.${v}`) })));
  const triggerOptions = $derived([
    { value: '', label: $t('histviz.trade.trigger.default') },
    ...opts.triggers.map((v) => ({ value: v, label: $t(`histviz.trade.trigger.${v}`) }))
  ]);
  const adaptiveOptions = $derived([
    { value: '', label: $t('histviz.trade.adaptive.off') },
    ...['patient', 'normal', 'urgent'].map((v) => ({ value: v, label: $t(`histviz.trade.adaptive.${v}`) }))
  ]);
  const sizingOptions = $derived(SIZINGS.map((v) => ({ value: v, label: $t(`histviz.trade.sizing.${v}`) })));
  const trailOptions = $derived(opts.trail.map((v) => ({ value: v, label: $t(`histviz.trade.trailKind.${v}`) })));

  /** The button's second line: what would be placed. */
  const summary = $derived.by(() => {
    const q = Number(orderQty) > 0 ? Number(orderQty) : 0;
    const sym = edit?.symbol ?? target?.symbol ?? '';
    const typ = $t(`histviz.trade.type.${type}`).toUpperCase();
    const p = (k) => (pos(px[k]) ? fmt(Number(px[k])) : '?');
    if (type === 'market') return `${q} ${sym} ${typ}`;
    if (type === 'limit') return `${q} ${sym} @ ${p('limit')} ${typ}`;
    if (type === 'stop') return `${q} ${sym} @ ${p('stop')} ${typ}`;
    if (type === 'stop_limit' || type === 'lit') return `${q} ${sym} @ ${p('stop')} / ${p('limit')} ${typ}`;
    if (type === 'mit') return `${q} ${sym} @ ${p('stop')} ${typ}`;
    if (type === 'trailing_stop') {
      const d = trailKind === 'percent' ? `${trailValue}%` : String(trailValue);
      return `${q} ${sym} ${typ} ${d}${trailStartOn && pos(px.stop) ? ` @ ${p('stop')}` : ''}`;
    }
    return `${q} ${sym} @ ${p('limit')} / ${p('stop')} ${typ}`;
  });

  function place() {
    error = '';
    const expires = tif === 'gtd' && expDate ? new Date(`${expDate}T${expTime || '00:00'}`).getTime() : null;
    if (sizing !== 'units' && !(orderQty > 0)) {
      error = $t(sizing === 'risk' && !slOn ? 'histviz.trade.err.riskNeedsStop' : 'histviz.trade.err.size');
      return;
    }
    const exits = opts.exits && type !== 'oco';
    const res = buildOrders(
      {
        side,
        type,
        qty: orderQty,
        limit: px.limit,
        stop: type === 'trailing_stop' && !(trailStartOn && opts.trailStart) ? null : px.stop,
        tif,
        expires,
        tp: tpOn && exits ? px.tp : null,
        sl: slOn && exits ? px.sl : null,
        reduceOnly: canReduce && reduceOnly,
        postOnly: opts.postOnly && postOnly,
        trail: type === 'trailing_stop' ? { kind: trailKind, value: trailValue } : null,
        display: opts.iceberg && display !== '' ? display : null,
        outsideRth: opts.outsideRth && outsideRth,
        trigger: opts.triggers.includes(trigger) ? trigger : null,
        closePosition: opts.closePosition && closePos,
        adaptive: opts.adaptive && adaptive ? adaptive : null,
        exitTif,
        exitsReduce: canReduce
      },
      { instrument: target?.instrument, symbol: target?.symbol, account: account?.id, connector: target?.connector ?? null, journal: journal || null, tifs }
    );
    if (res.error) {
      error = $t(`histviz.trade.err.${res.error}`);
      return;
    }
    oncreate?.(res.orders);
    protecting = null;
  }

  /** Tab badges: across the accounts the filter shows. */
  const shownBooks = $derived(tradeAccounts.filter((a) => !broker || a.id === broker).map((a) => accountBooks[a.id]?.book));
  const positionCount = $derived(shownBooks.reduce((n, b) => n + (b?.positions?.length ?? 0), 0));
  const orderCount = $derived(
    shownBooks.reduce((n, b) => n + (b?.orders?.length ?? 0), 0) + orders.filter((o) => !o.ref || o.status === 'sending').length
  );
</script>

{#snippet dockIcon(d)}
  {@const b = DOCK_BAND[d]}
  <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">
    <rect x={b.x} y={b.y} width={b.w} height={b.h} fill="currentColor" opacity="0.45" />
    <rect x="1.5" y="2.5" width="13" height="11" rx="2" fill="none" stroke="currentColor" stroke-width="1.2" />
  </svg>
{/snippet}

{#snippet priceBox(k, label, off)}
  <div class="box" class:off>
    {#if byOffset[k]}
      <input
        type="number"
        step={tick ? 1 : step}
        value={offsetOf(k)}
        oninput={(e) => setOffset(k, e.currentTarget.value)}
        disabled={off}
        aria-label={label}
      />
    {:else}
      <input type="number" min="0" {step} bind:value={px[k]} disabled={off} aria-label={label} />
    {/if}
    <span class="aside">{byOffset[k] ? (pos(px[k]) ? fmt(Number(px[k])) : '') : sideText(k)}</span>
    <button
      type="button"
      class="swap"
      title={$t(byOffset[k] ? 'histviz.trade.typePrice' : 'histviz.trade.typeDistance')}
      aria-label={$t(byOffset[k] ? 'histviz.trade.typePrice' : 'histviz.trade.typeDistance')}
      disabled={off}
      onclick={() => (byOffset[k] = !byOffset[k])}
    >
      <Icon name="repeat" size={12} />
    </button>
  </div>
{/snippet}

{#snippet exitField(k, label, on)}
  <div class="fld">
    <div class="lab-row">
      <span class="lab">{label}</span>
      <button type="button" class="sw" role="switch" aria-checked={on} aria-label={label} onclick={() => toggleExit(k)}></button>
    </div>
    {@render priceBox(k, label, !on)}
  </div>
{/snippet}

{#snippet toggle(label, hint, on, flipIt)}
  <div class="fld" title={$t(hint)}>
    <div class="lab-row">
      <span class="lab">{$t(label)}</span>
      <button type="button" class="sw" role="switch" aria-checked={on} aria-label={$t(label)} onclick={flipIt}></button>
    </div>
  </div>
{/snippet}

<svelte:window onresize={() => (dockOpen = false)} />

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<section
  bind:this={el}
  class="ticket dock-{dock}"
  class:row
  class:side={upright}
  class:collapsed={!expanded}
  style={expanded && size ? (row ? `height:${size}px` : `width:${size}px`) : ''}
  aria-label={$t('histviz.trade.title')}
  onpointerdown={(e) => e.stopPropagation()}
  onkeydown={(e) => e.stopPropagation()}
>
  {#if expanded}
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions, a11y_no_noninteractive_tabindex -->
    <div
      class="grip at-{gripEdge}"
      class:on={live != null}
      bind:this={grip}
      role="separator"
      aria-orientation={row ? 'horizontal' : 'vertical'}
      aria-label={$t('histviz.quick.resize')}
      title={$t('histviz.quick.resize')}
      tabindex="0"
      onpointerdown={onGripDown}
      onpointermove={onGripMove}
      onpointerup={onGripUp}
      onpointercancel={onGripUp}
      onkeydown={onGripKey}
    ></div>
  {/if}
  {#if upright && !expanded}
    <!-- Collapsed on a side: a strip the charts barely notice, reopened from its icon. -->
    <header class="strip">
      <button class="ibtn" title={$t('histviz.trade.expand')} aria-expanded="false" onclick={() => (expanded = true)}>
        <Icon name="receipt" size={13} />
      </button>
      <span class="vttl">{$t('histviz.trade.title')}</span>
      {#if orders.length}<span class="badge">{orders.length}</span>{/if}
      <button class="ibtn close" title={$t('common.close')} aria-label={$t('common.close')} onclick={() => onclose?.()}>
        <Icon name="x" size={12} />
      </button>
    </header>
  {:else}
  <header>
    <div class="hrow">
      <Icon name="receipt" size={13} />
      <strong class="ttl">{$t('histviz.trade.title')}</strong>
      <span class="sym" title={target?.instrument ?? ''}>{target?.symbol ?? $t('histviz.trade.noChart')}</span>
      <span class="grow"></span>
      {#if tradeAccounts.length && onkillswitch}
        <button
          type="button"
          class="kill"
          title={$t('histviz.trade.killHint')}
          aria-label={$t('histviz.trade.killHint')}
          disabled={killing}
          onclick={() => onkillswitch()}
        >
          <Icon name="stop" size={11} />
          {$t('histviz.trade.kill')}
        </button>
      {/if}
      <!-- Position: one button showing where it sits, the places it can take in a menu. -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <span
        class="dock"
        use:clickOutside={() => (dockOpen = false)}
        onkeydown={(e) => e.key === 'Escape' && ((dockOpen = false), dockBtn?.focus())}
      >
        <button
          type="button"
          class="ibtn dtrig"
          bind:this={dockBtn}
          title={$t('histviz.trade.position')}
          aria-label={$t('histviz.trade.position')}
          aria-haspopup="menu"
          aria-expanded={dockOpen}
          onclick={toggleDock}
        >
          {@render dockIcon(dock)}
          <Icon name="chevron-down" size={10} />
        </button>
        {#if dockOpen && dockAt}
          <div
            class="dmenu"
            role="menu"
            aria-label={$t('histviz.trade.position')}
            style:right="{dockAt.right}px"
            style:top={dockAt.top != null ? `${dockAt.top}px` : null}
            style:bottom={dockAt.bottom != null ? `${dockAt.bottom}px` : null}
          >
            {#each TICKET_DOCKS as d (d)}
              <button
                type="button"
                class="ditem"
                role="menuitemradio"
                aria-checked={dock === d}
                class:on={dock === d}
                disabled={!docks.includes(d)}
                onclick={() => pickDock(d)}
              >
                {@render dockIcon(d)}
                <span>{$t(`histviz.trade.dock.${d}`)}</span>
              </button>
            {/each}
          </div>
        {/if}
      </span>
      <button class="ibtn" title={expanded ? $t('histviz.trade.collapse') : $t('histviz.trade.expand')} aria-expanded={expanded} onclick={() => (expanded = !expanded)}>
        <Icon name={expanded ? 'chevrons-down-up' : 'chevrons-up-down'} size={12} />
      </button>
      <button class="ibtn close" title={$t('common.close')} aria-label={$t('common.close')} onclick={() => onclose?.()}>
        <Icon name="x" size={12} />
      </button>
    </div>
    <span class="ttabs" role="tablist">
      <button role="tab" aria-selected={tab === 'ticket'} class:on={tab === 'ticket'} onclick={() => ((tab = 'ticket'), (expanded = true))}>
        {$t('histviz.trade.tabTicket')}
      </button>
      <button role="tab" aria-selected={tab === 'positions'} class:on={tab === 'positions'} onclick={() => ((tab = 'positions'), (expanded = true))}>
        {$t('histviz.trade.tabPositions')}
        {#if positionCount}<span class="badge">{positionCount}</span>{/if}
      </button>
      <button role="tab" aria-selected={tab === 'orders'} class:on={tab === 'orders'} onclick={() => ((tab = 'orders'), (expanded = true))}>
        {$t('histviz.trade.tabOrders')}
        {#if orderCount}<span class="badge">{orderCount}</span>{/if}
      </button>
      <button role="tab" aria-selected={tab === 'account'} class:on={tab === 'account'} onclick={() => ((tab = 'account'), (expanded = true))}>
        {$t('histviz.trade.tabAccount')}
      </button>
      <button role="tab" aria-selected={tab === 'logs'} class:on={tab === 'logs'} onclick={() => ((tab = 'logs'), (expanded = true))}>
        {$t('histviz.trade.tabLogs')}
      </button>
    </span>
  </header>
  {#if halts.length}
    <div class="halts" role="status">
      {#each halts as h (h.id)}
        <p class="halt">
          <Icon name="alert-triangle" size={12} />
          <span>{haltText(h)}</span>
          {#if onlifthalt}
            <button type="button" class="ghost" onclick={() => onlifthalt(h.id)}>{$t('histviz.trade.lift')}</button>
          {/if}
        </p>
      {/each}
    </div>
  {/if}

  {#if expanded}
    <div class="body">
      {#if tab === 'ticket'}
        {#if edit}
          <p class="note edit">
            <Icon name="pencil" size={12} />
            {$t('histviz.trade.editing', { id: edit.ref, symbol: edit.symbol })}
            <button type="button" class="ghost" onclick={() => onendedit?.()}>{$t('histviz.trade.endEdit')}</button>
          </p>
        {/if}
        {#if protecting && !edit}
          <p class="note edit">
            <Icon name="shield" size={12} />
            {$t('histviz.trade.protecting', { side: $t(`histviz.book.side.${protecting.side === 'short' ? 'short' : 'long'}`), qty: protecting.qty, symbol: protecting.symbol })}
            <button type="button" class="ghost" onclick={() => (protecting = null)}>{$t('histviz.trade.endEdit')}</button>
          </p>
        {/if}
        {#if !account && !edit}
          <p class="note warn"><Icon name="plug" size={12} /> {lock || $t('histviz.trade.lock.noAccount')}</p>
        {:else if !target && !edit}
          <p class="note">{$t('histviz.trade.pickChart')}</p>
        {:else}
          <div class="form">
            <!-- Side at the market, then the type. -->
            <div class="sec">
              <div class="sides" role="radiogroup" aria-label={$t('histviz.trade.side')}>
                <button type="button" role="radio" aria-checked={side === 'sell'} class="sell" class:on={side === 'sell'} disabled={!!edit} onclick={() => (side = 'sell')}>
                  <span class="s-lab">{$t('histviz.trade.sell')}</span>
                  <span class="s-px">{bidPx ? fmt(bidPx) : ''}</span>
                </button>
                <button type="button" role="radio" aria-checked={side === 'buy'} class="buy" class:on={side === 'buy'} disabled={!!edit} onclick={() => (side = 'buy')}>
                  <span class="s-lab">{$t('histviz.trade.buy')}</span>
                  <span class="s-px">{askPx ? fmt(askPx) : ''}</span>
                </button>
                {#if spread != null}<span class="spread" title={$t('histviz.trade.spread')}>{fmt(spread)}</span>{/if}
              </div>
              <div class="types" role="tablist" aria-label={$t('histviz.trade.typeLabel')}>
                {#each types as v (v)}
                  <button type="button" role="tab" aria-selected={type === v} class:on={type === v} disabled={!!edit && type !== v} onclick={() => (type = v)}>
                    {$t(`histviz.trade.type.${v}`)}
                  </button>
                {/each}
              </div>
            </div>

            <!-- The prices the type carries, and the size. -->
            <div class="sec">
              {#if need.limit}
                <div class="fld">
                  <span class="lab">{$t('histviz.trade.limitPrice')}</span>
                  {@render priceBox('limit', $t('histviz.trade.limitPrice'), false)}
                </div>
              {/if}
              {#if type === 'trailing_stop' && !edit}
                <div class="fld">
                  <span class="lab">{$t('histviz.trade.trailBy')}</span>
                  <div class="pick">
                    <div class="box">
                      <input type="number" min="0" step="any" bind:value={trailValue} aria-label={$t('histviz.trade.trailBy')} />
                      <span class="aside">{trailKind === 'percent' ? '%' : $t('histviz.trade.trailUnit')}</span>
                    </div>
                    {#if trailOptions.length > 1}
                      <Dropdown bind:value={trailKind} options={trailOptions} ariaLabel={$t('histviz.trade.trailBy')} float />
                    {/if}
                  </div>
                </div>
              {/if}
              {#if showStop}
                {#if type === 'trailing_stop'}
                  <div class="fld" title={$t('histviz.trade.trailStartHint')}>
                    <div class="lab-row">
                      <span class="lab">{$t(stopLabel)}</span>
                      <button type="button" class="sw" role="switch" aria-checked={trailStartOn} aria-label={$t(stopLabel)} disabled={!!edit} onclick={() => (trailStartOn = !trailStartOn)}></button>
                    </div>
                    {@render priceBox('stop', $t(stopLabel), !trailStartOn)}
                  </div>
                {:else}
                  <div class="fld">
                    <span class="lab">{$t(stopLabel)}</span>
                    {@render priceBox('stop', $t(stopLabel), false)}
                  </div>
                {/if}
              {/if}
              <SizeField
                label={$t('histviz.trade.qty')}
                bind:sizing
                bind:qty
                bind:amount
                options={sizingOptions}
                locked={!!edit}
                sized={orderQty > 0 ? $t('histviz.trade.sizedUnits', { n: orderQty }) : sizing === 'risk' && !slOn ? $t('histviz.trade.err.riskNeedsStop') : ''}
                amountHint={sizing === 'units' ? '' : $t(`histviz.trade.sizingHint.${sizing}`)}
                amountLabel={$t(`histviz.trade.sizing.${sizing}`)}
              />
            </div>

            {#if type !== 'oco' && !edit && opts.exits}
              <div class="sec">
                <button type="button" class="sec-h" aria-expanded={exitsOpen || row} disabled={row} onclick={() => (exitsOpen = !exitsOpen)}>
                  <span>{$t('histviz.trade.exits')}</span>
                  {#if !row}<Icon name={exitsOpen ? 'chevron-up' : 'chevron-down'} size={12} />{/if}
                </button>
                {#if exitsOpen || row}
                  {@render exitField('tp', $t('histviz.trade.takeProfit'), tpOn)}
                  {@render exitField('sl', $t('histviz.trade.stopLoss'), slOn)}
                  {#if (tpOn || slOn) && exitTifOptions.length > 1}
                    <div class="fld">
                      <span class="lab">{$t('histviz.trade.exitTif')}</span>
                      <Dropdown bind:value={exitTif} options={exitTifOptions} ariaLabel={$t('histviz.trade.exitTif')} float />
                    </div>
                  {/if}
                {/if}
              </div>
            {/if}

            <div class="sec">
              <button type="button" class="sec-h" aria-expanded={extraOpen || row} disabled={row} onclick={() => (extraOpen = !extraOpen)}>
                <span>{$t('histviz.trade.extra')}</span>
                {#if !row}<Icon name={extraOpen ? 'chevron-up' : 'chevron-down'} size={12} />{/if}
              </button>
              {#if extraOpen || row}
                <div class="fld">
                  <span class="lab">{$t('histviz.trade.tifLabel')}</span>
                  <Dropdown bind:value={tif} options={tifOptions} ariaLabel={$t('histviz.trade.tifLabel')} float />
                </div>
                {#if target?.leverage != null}
                  <div class="fld" title={$t('histviz.trade.leverageHint')}>
                    <span class="lab">{$t('histviz.trade.leverage')}</span>
                    <div class="box ro">
                      <span class="lever">{target.leverage}×</span>
                      {#if target.marginMode}<span class="aside">{$t(`histviz.trade.marginMode.${target.marginMode}`)}</span>{/if}
                    </div>
                  </div>
                {/if}
                {#if tif === 'gtd'}
                  <div class="pair">
                    <input type="date" bind:value={expDate} aria-label={$t('histviz.trade.expiryDate')} />
                    <input type="time" bind:value={expTime} aria-label={$t('histviz.trade.expiryTime')} />
                  </div>
                {/if}
                {#if canReduce && !edit && !(closePos && opts.closePosition && (type === 'stop' || type === 'mit'))}
                  {@render toggle('histviz.trade.reduceOnly', 'histviz.trade.reduceOnlyHint', reduceOnly, () => (reduceOnly = !reduceOnly))}
                {/if}
                {#if opts.postOnly && type === 'limit' && !edit}
                  {@render toggle('histviz.trade.postOnly', 'histviz.trade.postOnlyHint', postOnly, () => (postOnly = !postOnly))}
                {/if}
                {#if opts.iceberg && need.limit && type !== 'oco' && !edit}
                  <div class="fld" title={$t('histviz.trade.icebergHint')}>
                    <span class="lab">{$t('histviz.trade.iceberg')}</span>
                    <div class="box">
                      <input type="number" min="0" step="any" bind:value={display} placeholder={$t('histviz.trade.icebergOff')} aria-label={$t('histviz.trade.iceberg')} />
                      <span class="aside">{$t('histviz.trade.units')}</span>
                    </div>
                  </div>
                {/if}
                {#if opts.triggers.length && (triggeredType(type) || type === 'oco') && !edit}
                  <div class="fld" title={$t('histviz.trade.triggerHint')}>
                    <span class="lab">{$t('histviz.trade.triggerLabel')}</span>
                    <Dropdown bind:value={trigger} options={triggerOptions} ariaLabel={$t('histviz.trade.triggerLabel')} float />
                  </div>
                {/if}
                {#if opts.closePosition && (type === 'stop' || type === 'mit') && !edit}
                  {@render toggle('histviz.trade.closePos', 'histviz.trade.closePosHint', closePos, () => (closePos = !closePos))}
                {/if}
                {#if opts.outsideRth && !edit}
                  {@render toggle('histviz.trade.outsideRth', 'histviz.trade.outsideRthHint', outsideRth, () => (outsideRth = !outsideRth))}
                {/if}
                {#if opts.adaptive && (type === 'market' || type === 'limit') && !edit}
                  <div class="fld" title={$t('histviz.trade.adaptiveHint')}>
                    <span class="lab">{$t('histviz.trade.adaptiveLabel')}</span>
                    <Dropdown bind:value={adaptive} options={adaptiveOptions} ariaLabel={$t('histviz.trade.adaptiveLabel')} float />
                  </div>
                {/if}
              {/if}
              {#if journals && !edit}
                <div class="fld">
                  <span class="lab">{$t('histviz.trade.journal')}</span>
                  <div class="pick">
                    <Dropdown value={journal} options={journalOptions} ariaLabel={$t('histviz.trade.journal')} onpick={pickJournal} disabled={!linked} float />
                    <button type="button" class="add" title={$t('histviz.trade.newJournal')} aria-label={$t('histviz.trade.newJournal')} onclick={openNewJournal}>
                      <Icon name="plus" size={13} />
                    </button>
                  </div>
                </div>
              {/if}
            </div>

            <div class="sec go">
              {#if edit}
                <button type="button" class="place {side}" onclick={amend}>
                  <strong>{$t('histviz.trade.modify', { id: edit.ref })}</strong>
                  <span>{summary}</span>
                </button>
              {:else}
                <button type="button" class="place {side}" onclick={place}>
                  <strong>{$t(side === 'buy' ? 'histviz.trade.buy' : 'histviz.trade.sell')}</strong>
                  <span>{summary}</span>
                </button>
              {/if}
              {#if error}<p class="note err">{error}</p>{/if}
            </div>
          </div>
        {/if}
      {:else}
        <BrokerTabs
          {tab}
          accounts={tradeAccounts}
          books={accountBooks}
          {livePrices}
          {trailPrices}
          marks={accountMarks}
          symbol={target?.symbol ?? ''}
          bind:broker
          {orders}
          oncancelline={(id) => oncancel?.(id)}
          {oncancelbroker}
          {onresendunsent}
          {ondismissunsent}
          {oncancelall}
          {oneditline}
          {oneditbroker}
          {oncloseposition}
          {onprotectposition}
        />
      {/if}
    </div>
  {/if}
  {/if}
</section>

<Modal bind:open={newOpen} title={$t('histviz.trade.newJournal')} size="sm">
  <form
    class="new-journal"
    onsubmit={(e) => {
      e.preventDefault();
      createJournal();
    }}
  >
    <label class="nj-field">
      <span>{$t('histviz.trade.journalName')}</span>
      <!-- svelte-ignore a11y_autofocus -->
      <input bind:value={newName} autofocus placeholder={$t('histviz.trade.journalNamePlaceholder')} />
    </label>
    {#if newErr}<p class="note err">{newErr}</p>{/if}
    <div class="nj-actions">
      <button type="button" class="ghost" onclick={() => (newOpen = false)}>{$t('common.cancel')}</button>
      <button type="submit" class="primary" disabled={newBusy}>{$t('histviz.trade.createJournal')}</button>
    </div>
  </form>
</Modal>

<style>
  /* In the flow, not over the charts: the page gives it a slot and the grid takes the rest. */
  .ticket {
    position: relative;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    max-height: 100%;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    font-size: var(--text-xs);
    overflow: hidden;
  }
  .ticket.row {
    width: 100%;
  }
  /* The edge facing the charts: the same grab strip as the quick-backtest panel. */
  .grip {
    position: absolute;
    z-index: 3;
    touch-action: none;
  }
  .grip.at-top,
  .grip.at-bottom {
    left: 0;
    right: 0;
    height: 6px;
    cursor: row-resize;
  }
  .grip.at-top {
    top: 0;
  }
  .grip.at-bottom {
    bottom: 0;
  }
  .grip.at-left,
  .grip.at-right {
    top: 0;
    bottom: 0;
    width: 6px;
    cursor: col-resize;
  }
  .grip.at-left {
    left: 0;
  }
  .grip.at-right {
    right: 0;
  }
  .grip::after {
    content: '';
    position: absolute;
    background: var(--accent);
    opacity: 0;
    transition: opacity var(--dur-fast) var(--ease);
  }
  .grip:hover::after,
  .grip:focus-visible::after,
  .grip.on::after {
    opacity: 1;
  }
  .grip.at-top::after,
  .grip.at-bottom::after {
    left: 50%;
    width: 44px;
    height: 2px;
    transform: translateX(-50%);
  }
  .grip.at-top::after {
    top: 2px;
  }
  .grip.at-bottom::after {
    bottom: 2px;
  }
  .grip.at-left::after,
  .grip.at-right::after {
    top: 50%;
    width: 2px;
    height: 44px;
    transform: translateY(-50%);
  }
  .grip.at-left::after {
    left: 2px;
  }
  .grip.at-right::after {
    right: 2px;
  }
  .grip:focus-visible {
    outline: none;
  }
  /* Open along an edge, it keeps one height across its tabs: switching from the ticket to a
     short log must not move the charts beside it. Longer content scrolls in the body. */
  .ticket.row:not(.collapsed) {
    height: min(280px, 40vh);
  }
  .ticket.side {
    width: 300px;
    height: 100%;
  }
  /* Between two rows of charts it must leave them their height. */
  .dock-center-h {
    max-height: 40vh;
  }
  .ticket.side.collapsed {
    width: 32px;
  }
  .strip {
    flex-direction: column;
    align-items: center;
    flex-wrap: nowrap;
    height: 100%;
    padding: var(--space-1) 0;
    border-bottom: none;
  }
  .strip .close {
    margin-top: auto;
  }
  .vttl {
    writing-mode: vertical-rl;
    color: var(--muted);
    font-weight: 600;
  }

  /* Two lines: title, symbol and the controls, then the tabs. */
  header {
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: var(--space-1);
    padding: var(--space-1) var(--space-2);
    border-bottom: 1px solid var(--border);
    background: var(--surface-2);
  }
  .collapsed header {
    border-bottom: none;
  }
  .ttl {
    font-weight: 600;
  }
  .hrow {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }
  .sym {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--mono);
    color: var(--muted);
  }
  .grow {
    flex: 1;
  }
  /* Not the app's `.tabs`: that one is an underlined page strip, this is a pill pair in a
     header. */
  .ttabs {
    display: flex;
    gap: 2px;
  }
  .ttabs button {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    padding: 2px var(--space-2);
    border: 1px solid transparent;
    border-radius: var(--radius);
    background: none;
    color: var(--muted);
    font-size: var(--text-xs);
    cursor: pointer;
  }
  .ttabs button.on {
    color: var(--text);
    border-color: var(--border);
    background: var(--surface);
  }
  .badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    box-sizing: border-box;
    min-width: 14px;
    height: 14px;
    padding: 0 3px;
    border-radius: 7px;
    background: var(--accent);
    color: #fff;
    font-size: 10px;
    font-variant-numeric: tabular-nums;
    line-height: 1;
  }
  /* Position picker: the current place on the button, every place in its menu. */
  .dock {
    display: inline-flex;
  }
  .ibtn.dtrig {
    width: auto;
    gap: 2px;
    padding: 0 3px;
  }
  .dmenu {
    position: fixed;
    z-index: 50;
    display: flex;
    flex-direction: column;
    min-width: 200px;
    padding: var(--space-1);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: 0 6px 20px rgba(0, 0, 0, 0.25);
  }
  .ditem {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-2);
    border: none;
    border-radius: var(--radius);
    background: none;
    color: var(--text);
    font-size: var(--text-xs);
    text-align: left;
    cursor: pointer;
  }
  .ditem svg {
    flex: none;
    color: var(--muted);
  }
  .ditem:hover:not(:disabled) {
    background: var(--surface-2);
  }
  .ditem.on,
  .ditem.on svg {
    color: var(--accent);
  }
  .ditem:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .ibtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    padding: 0;
    border: none;
    border-radius: var(--radius);
    background: none;
    color: var(--muted);
    cursor: pointer;
  }
  .ibtn:hover {
    background: var(--surface);
    color: var(--text);
  }
  .ibtn.close:hover {
    color: var(--red);
  }
  /* The global kill switch: always in reach while an account trades, red like a stop. */
  .kill {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    height: 22px;
    padding: 0 var(--space-2);
    border: 1px solid color-mix(in srgb, var(--red) 55%, var(--border));
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--red) 8%, transparent);
    color: var(--red);
    font-size: var(--text-xs);
    font-weight: 600;
    cursor: pointer;
  }
  .kill:hover:not(:disabled) {
    background: color-mix(in srgb, var(--red) 16%, transparent);
  }
  .kill:disabled {
    opacity: 0.5;
    cursor: default;
  }
  /* A halt in force on the chart's account: under the header, whatever the tab. */
  .halts {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: var(--space-1) var(--space-2);
    border-bottom: 1px solid var(--border);
    background: color-mix(in srgb, var(--amber) 10%, var(--surface));
  }
  .halt {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: 0;
    color: var(--amber);
  }
  .halt span {
    flex: 1;
    min-width: 0;
    color: var(--text);
  }

  .body {
    overflow: auto;
    padding: var(--space-3);
  }

  /* ── Form: sections stacked on a side, side by side along an edge ── */
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .row .form {
    flex-direction: row;
    flex-wrap: wrap;
    align-items: stretch;
    gap: var(--space-4);
  }
  .sec {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 0;
  }
  .side .sec + .sec {
    padding-top: var(--space-3);
    border-top: 1px solid var(--border);
  }
  .row .sec {
    flex: 1 1 200px;
    max-width: 300px;
  }
  .row .sec + .sec {
    padding-left: var(--space-4);
    border-left: 1px solid var(--border);
  }
  .row .sec.go {
    flex: 0 0 190px;
    justify-content: flex-end;
  }
  .sec-h {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0;
    border: none;
    background: none;
    color: var(--text);
    font-size: var(--text-xs);
    font-weight: 600;
    cursor: pointer;
  }
  .sec-h:disabled {
    cursor: default;
    opacity: 1;
  }

  /* Sell | Buy at the market, the spread riding on the seam. */
  .sides {
    position: relative;
    display: grid;
    grid-template-columns: 1fr 1fr;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .sides button {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: var(--space-2) var(--space-3);
    border: none;
    background: var(--surface-2);
    color: var(--muted);
    cursor: pointer;
    text-align: left;
  }
  .sides .buy {
    text-align: right;
    align-items: flex-end;
  }
  .sides .sell + .buy {
    border-left: 1px solid var(--border);
  }
  .s-lab {
    font-weight: 600;
  }
  .s-px {
    min-height: 1.2em;
    font-family: var(--mono);
    font-size: var(--text-sm);
  }
  .sides .sell.on {
    background: color-mix(in srgb, var(--red) 18%, var(--surface));
    color: var(--red);
  }
  .sides .buy.on {
    background: color-mix(in srgb, var(--accent) 18%, var(--surface));
    color: var(--accent);
  }
  .spread {
    position: absolute;
    top: var(--space-1);
    left: 50%;
    transform: translateX(-50%);
    padding: 0 var(--space-1);
    border-radius: var(--radius);
    background: var(--surface);
    color: var(--muted);
    font-family: var(--mono);
    font-size: 10px;
    pointer-events: none;
  }

  /* Order types: an underlined strip, the chosen one marked. */
  .types {
    display: flex;
    border-bottom: 1px solid var(--border);
  }
  .types button {
    flex: 1 1 auto;
    padding: var(--space-1) 2px;
    border: none;
    border-bottom: 2px solid transparent;
    margin-bottom: -1px;
    background: none;
    color: var(--muted);
    font-size: var(--text-xs);
    white-space: nowrap;
    cursor: pointer;
  }
  .types button.on {
    color: var(--text);
    border-bottom-color: var(--accent);
  }

  .fld {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .lab {
    color: var(--muted);
  }
  .lab-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }
  /* Read from the venue, never typed: the box shows it, nothing to focus. */
  .box.ro {
    background: var(--surface-2);
    cursor: default;
  }
  .lever {
    flex: 1;
    font-family: var(--mono);
  }

  /* One frame per field: the input inside is borderless, the box owns the hairline and the
     focus color. */
  .box {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    height: var(--control-h, 30px);
    padding: 0 var(--space-2);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
    background: var(--surface);
    transition: border-color var(--dur-fast) var(--ease);
  }
  .box:focus-within {
    border-color: var(--accent);
  }
  .box.off {
    opacity: 0.55;
  }
  .box input {
    flex: 1 1 0;
    min-width: 0;
    height: 100%;
    padding: 0;
    border: none;
    background: transparent;
    box-shadow: none;
    font-family: var(--mono);
  }
  .swap {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    padding: 0;
    border: none;
    border-radius: var(--radius);
    background: none;
    color: var(--muted);
    cursor: pointer;
  }
  .swap:hover:not(:disabled) {
    background: var(--surface-2);
    color: var(--text);
  }
  /* Sized to its text, so the input takes the rest of the box and the buttons sit at its ends. */
  .aside {
    flex: 0 1 auto;
    min-width: 0;
    max-width: 50%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: right;
    color: var(--muted);
    font-family: var(--mono);
  }

  /* On/off for an exit. */
  .sw {
    position: relative;
    width: 28px;
    height: 16px;
    padding: 0;
    border: none;
    border-radius: 8px;
    background: var(--border-control);
    cursor: pointer;
    transition: background var(--dur-fast) var(--ease);
  }
  .sw::after {
    content: '';
    position: absolute;
    top: 2px;
    left: 2px;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: #fff;
    transition: transform var(--dur-fast) var(--ease);
  }
  .sw[aria-checked='true'] {
    background: var(--accent);
  }
  .sw[aria-checked='true']::after {
    transform: translateX(12px);
  }

  .pair {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-2);
  }
  .pair input {
    min-width: 0;
    font-size: var(--text-xs);
  }
  .pick {
    display: flex;
    gap: var(--space-1);
    min-width: 0;
  }
  .pick > :global(:first-child) {
    flex: 1;
    min-width: 0;
  }
  .add {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: var(--control-h, 30px);
    padding: 0;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
    background: var(--surface);
    color: var(--muted);
    cursor: pointer;
  }
  .add:hover {
    border-color: var(--accent);
    color: var(--accent);
  }

  .place {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    width: 100%;
    padding: var(--space-2) var(--space-3);
    border: none;
    border-radius: var(--radius);
    color: #fff;
    cursor: pointer;
  }
  .place strong {
    font-size: var(--text-sm);
  }
  .place span {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--mono);
    opacity: 0.9;
  }
  .place.buy {
    background: var(--accent);
  }
  .place.sell {
    background: var(--red);
  }
  .place:hover {
    filter: brightness(1.08);
  }
  .note {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    margin: 0;
    color: var(--muted);
  }
  .note.warn {
    color: var(--amber);
  }
  .note.err {
    color: var(--red);
  }
  .note.edit {
    color: var(--amber);
    margin-bottom: var(--space-2);
  }
  .note.edit .ghost {
    margin-left: auto;
  }

  .new-journal {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .nj-field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .nj-field span {
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .nj-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }

  .n {
    text-align: right;
  }
  .mono {
    font-family: var(--mono);
  }
  .muted {
    color: var(--muted);
  }
  .tag {
    padding: 0 3px;
    border: 1px solid var(--border);
    border-radius: 3px;
    color: var(--muted);
    font-size: 10px;
  }
  .list-foot {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }
  .ghost {
    padding: 2px var(--space-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: none;
    color: var(--text);
    font-size: var(--text-xs);
    cursor: pointer;
  }
  .ghost:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
</style>
