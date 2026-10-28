<script>
  // Charts (module id `histviz`): a workspace of chart panes.
  //
  // The page owns three things and nothing else:
  //   - the **grid**: rows x columns (1x1 up to 3x4) with draggable splitters, so a vertical
  //     split, a horizontal one and a 2x2 come out of one model instead of a list of named
  //     layouts;
  //   - the **rail**: the instrument lists, from the chart's own and from the Watchlists
  //     module (see WatchRail);
  //   - the **live connection**: one SSE stream for every pane on screen. The server keys its
  //     sockets by connector, so four panes on one Alpaca key are one socket there and one
  //     connection here; each event carries the pane it belongs to.
  //
  // Everything about *a* chart (its instrument, its window, its studies, its drawings, its
  // quick session) belongs to Pane.svelte. A pane knows nothing about its neighbours.
  import { onMount, tick, untrack } from 'svelte';
  import { page } from '$app/stores';
  import { goto, beforeNavigate } from '$app/navigation';
  import Icon from '$lib/ui/Icon.svelte';
  import { chrome } from '$lib/ui/chrome.svelte.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { toast } from '$lib/ui/toast.svelte.js';
  import Button from '$lib/ui/Button.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import ChoiceModal from '$lib/ui/ChoiceModal.svelte';
  import PromptModal from '$lib/ui/PromptModal.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import { clickOutside } from '$lib/ui/clickOutside.js';
  import { quickTitles } from '$lib/ui/quickTitles.js';
  import RequireModule from '$lib/modules/RequireModule.svelte';
  import { histvizApi } from '$lib/modules/histviz/api.js';
  import { connectorsApi } from '$lib/connectors/api.js';
  import Pane from '$lib/modules/histviz/Pane.svelte';
  import WatchRail from '$lib/modules/histviz/WatchRail.svelte';
  import QuickPanel from '$lib/modules/histviz/QuickPanel.svelte';
  import BookPanel from '$lib/modules/histviz/BookPanel.svelte';
  import OrderTicket from '$lib/modules/histviz/OrderTicket.svelte';
  import { loadOrders, saveOrders, cancelOrder, moveOrder, legsOf, gone, dec, reissue, TICKET_DOCKS } from '$lib/modules/histviz/orders.js';
  import { brokersApi, haltsOn } from '$lib/brokers/api.js';
  import { poll } from '$lib/brokers/poll.js';
  import InstrumentMetaModal from '$lib/script/InstrumentMetaModal.svelte';
  import { instrumentMeta } from '$lib/script/api.js';
  import { applyEvent } from '$lib/modules/histviz/accountEvents.js';
  import {
    MODULE as BOOK_MODULE,
    tradeLock
  } from '$lib/modules/histviz/book.js';
  import { DEFAULT_SETTINGS, normalizeSettings } from '$lib/modules/histviz/settings.js';
  import { liveStream } from '$lib/live/stream.js';
  import {
    MAX_ROWS,
    MAX_COLS,
    fromRow,
    toBody,
    savedKey,
    migrateLegacy,
    newWorkspace,
    newPane,
    reshape,
    resizeTrack,
    visiblePanes
  } from '$lib/modules/histviz/workspace.js';
  import { SLICE_BARS, loadDrawings, instrumentKeyOf } from '$lib/modules/histviz/pane.js';
  import { templateOf, templatesIn } from '$lib/modules/histviz/templates.js';
  import { withoutIndicator } from '$lib/modules/histviz/indicators.js';
  import { onIndicatorRemoved } from '$lib/indicators/api.js';

  // The workspace this browser had open, so a reload lands where you left off even before the
  // server list comes back.
  const LAST_KEY = 'otw.histviz.workspace.v1';
  /** What the single-chart page kept in this browser. Read once, so upgrading lands on the
   *  instrument you were looking at rather than on an empty pane. */
  const LEGACY_KEY = 'otw.histviz.session.v2';

  function lastSingleChart() {
    try {
      return JSON.parse(localStorage.getItem(LEGACY_KEY) || '{}')?.coords ?? null;
    } catch {
      return null;
    }
  }

  let ws = $state(newWorkspace());
  let workspaces = $state([]);
  let datasets = $state([]);
  let providers = $state([]);
  let connectors = $state([]);
  let settings = $state({ ...DEFAULT_SETTINGS });
  let settingsLoaded = $state(false);
  let railOpen = $state(localStorage.getItem('otw.histviz.railOpen') !== '0');
  // The rail/charts split, dragged on the hairline between them and kept across sessions.
  const RAIL_KEY = 'otw.histviz.railW';
  const RAIL_MIN = 150;
  const RAIL_MAX = 480;
  const clampRail = (w) => Math.min(RAIL_MAX, Math.max(RAIL_MIN, Math.round(w)));
  let railW = $state(clampRail(Number(localStorage.getItem(RAIL_KEY)) || 198));
  let maximized = $state(null);
  let error = $state('');
  let gridEl = $state(null);
  let barDock = $state(null);
  let actDock = $state(null);
  let rangeDock = $state(null);
  let ctlDock = $state(null);
  let railDock = $state(null);
  let railRef = $state(null);
  /** Renaming swaps the picker for a field **in the same slot**: one control, one width,
   *  so the bar never reflows and the name is one click away instead of always in the way. */
  let renaming = $state(false);
  let nameEl = $state(null);
  /** The cell the pointer is over in the shape grid: the picker previews the layout it would
   *  apply before the click, which is what makes a 12-cell grid readable without labels. */
  let shapeHover = $state(null);
  let shapeOpen = $state(false);
  let confirmDelete = $state(false);
  let revertOpen = $state(false);
  /** The workspace the delete confirmation is about: { id, name }. */
  let deleting = $state(null);
  // The workspace menu: opened under its button, drawn fixed (the bar is a scroller).
  let wsMenuOpen = $state(false);
  let wsMenuAt = $state(null);
  let wsBtn = $state(null);
  function toggleWsMenu() {
    if (wsMenuOpen) {
      wsMenuOpen = false;
      return;
    }
    const r = wsBtn?.getBoundingClientRect();
    wsMenuAt = r ? { top: r.bottom + 4, left: r.left, width: Math.max(r.width, 240) } : null;
    wsMenuOpen = true;
  }
  function renameFromMenu(id) {
    wsMenuOpen = false;
    if (!id || id === ws.id) return startRename();
    guard(async () => {
      await switchWorkspace(id);
      startRename();
    });
  }
  function askDelete(id, name) {
    wsMenuOpen = false;
    deleting = { id, name };
    confirmDelete = true;
  }

  // ── Broker book ──
  // One account per chart (`pane.broker`, saved with the workspace): a chart on IBKR futures
  // and one on a crypto venue are traded through different accounts. The button in the bar
  // works on the active chart, like every control there. The pull is explicit and its result
  // is a snapshot per chart, never a subscription, and no call from here can place, change or
  // cancel anything.
  let bookOpen = $state(false);
  let bookAccounts = $state([]);
  let bookError = $state('');
  /** paneId -> { book, at, busy, error } */
  let books = $state({});

  // ── Quick backtest ──
  // One session for the whole workspace. Every pane replays its own fills (its bars are what
  // MAE and R are measured against) and publishes the result here, so a user can click
  // entries on three instruments and read one set of numbers.
  const QUICK_KEY = 'otw.histviz.quick';
  let quickOn = $state(localStorage.getItem(QUICK_KEY) === '1');
  let quickExpanded = $state(false);
  let quickHeight = $state(240);
  let quickTab = $state('trades');
  let quickSessions = $state({});

  // ── Chart trading ──
  // Draft orders for the whole workspace, drawn by the pane showing their instrument and
  // listed in the one ticket. A chart is tradable through its own account, when trading is
  // switched on for it (the `trading` grant) and the account's broker serves the chart's
  // instrument: otherwise no `+` and no quick buttons, and the chart says which is missing.
  const TICKET_KEY = 'otw.histviz.ticket.v1';
  function loadTicketUi() {
    const ui = { open: false, dock: 'bottom', expanded: true, tab: 'ticket', journal: '', broker: '', sizes: {} };
    try {
      const saved = JSON.parse(localStorage.getItem(TICKET_KEY) || '{}');
      if (typeof saved.open === 'boolean') ui.open = saved.open;
      if (TICKET_DOCKS.includes(saved.dock)) ui.dock = saved.dock;
      if (typeof saved.expanded === 'boolean') ui.expanded = saved.expanded;
      if (['ticket', 'positions', 'orders', 'account', 'logs'].includes(saved.tab)) ui.tab = saved.tab;
      if (typeof saved.broker === 'string') ui.broker = saved.broker;
      if (typeof saved.journal === 'string') ui.journal = saved.journal;
      for (const d of TICKET_DOCKS) if (Number(saved.sizes?.[d]) > 0) ui.sizes[d] = Number(saved.sizes[d]);
    } catch {
      /* defaults */
    }
    return ui;
  }
  let orders = $state(loadOrders());
  let ticketUi = $state(loadTicketUi());
  let ticketPrefill = $state(null);
  /** Each pane's market, for the ticket's prices: paneId -> { last, digits, bid, ask, tick }. */
  let markets = $state({});
  const accountOf = (p) => bookAccounts.find((a) => a.id === p?.broker) ?? null;
  /** The account a chart trades through, or null with the reason in `tradeLockOf`. */
  const tradeAccountOf = (p) => (tradeLock(accountOf(p), p?.coords) ? null : accountOf(p));
  const tradeLockOf = (p) => tradeLock(accountOf(p), p?.coords);
  /** The lock as the chart and the ticket say it. */
  function lockText(p) {
    const k = tradeLockOf(p);
    return k ? $t(`histviz.trade.lock.${k}`, { broker: accountOf(p)?.capability?.label ?? '' }) : '';
  }

  $effect(() => saveOrders($state.snapshot(orders)));
  $effect(() => {
    try {
      localStorage.setItem(TICKET_KEY, JSON.stringify($state.snapshot(ticketUi)));
    } catch {
      /* non-fatal */
    }
  });

  // ── Beacons ──
  // Every order action and every fill says how it went in a short note at the bottom right,
  // gone after BEACON_MS: placed, moved, cancelled, filled, refused.
  const BEACON_MS = 3000;
  function beacon(kind, key, params) {
    toast.push(kind, $t(`histviz.trade.beacon.${key}`, params), BEACON_MS);
  }
  /** How a beacon names an order: side, size, instrument and type. */
  function orderName(o) {
    const sym = o.symbol || o.instrument || '';
    return [String(o.side ?? '').toUpperCase(), o.qty, sym, String(o.type ?? o.order_type ?? '').replace('_', ' ')].filter((x) => x !== '' && x != null).join(' ');
  }
  /** An order the broker names by id: this page's line when it drew one, else the book's row. */
  function brokerOrder(account, id) {
    return (
      orders.find((o) => o.account === account && String(o.ref) === String(id)) ??
      (accountBooks[account]?.book?.orders ?? []).find((o) => String(o.id) === String(id)) ??
      null
    );
  }
  /** account|id of orders this page cancelled: the broker's own word on them says nothing new. */
  const cancelledHere = new Set();
  /** `account|ref` of orders the stream said were done while a drag was moving them: the
   *  move's answer decides (a broker that changes an order as a new one names it there). */
  const doneWhileMoving = new Set();

  /** The chart's last price for an instrument, what a price collar measures from: the active
   *  chart's when it shows it, else any chart's that does. Null when none does. */
  /** The chart's last price for an instrument and when it was seen, from the pane showing it. */
  function referenceOf(instrument) {
    const shows = (p) => p?.coords && instrumentKeyOf(p.coords) === instrument && markets[p.id]?.last > 0;
    const p = shows(activePane) ? activePane : ws.panes.find(shows);
    return p ? { price: markets[p.id].last, at: markets[p.id].lastAt ?? null } : { price: null, at: null };
  }

  /** Orders made together go to their account as one ticket: drawn at once as `sending`,
   *  `working` with the broker's id once it accepted every leg, gone with the reason if not.
   *  `confirmDuplicate`: the user said to send an order like one sent seconds ago. */
  async function addOrders(list, err, { confirmDuplicate = false } = {}) {
    if (err) {
      error = $t(`histviz.trade.err.${err}`);
      return;
    }
    if (!list?.length) return;
    const ref = referenceOf(list[0].instrument);
    const ticket = legsOf(list, { reference: ref.price, referenceAt: ref.at, confirmDuplicate });
    const account = list[0].account;
    if (!account || !ticket) {
      orders = [...orders, ...list];
      return;
    }
    const keys = new Set(list.map((o) => o.id));
    orders = [...orders, ...list.map((o) => ({ ...o, status: 'sending' }))];
    try {
      const placed = await brokersApi.placeOrders(account, ticket, BOOK_MODULE);
      const by = Object.fromEntries(placed.map((p) => [p.key, p]));
      const sentAt = Date.now();
      orders = orders.map((o) => (keys.has(o.id) ? { ...o, status: 'working', ref: by[o.id]?.id ?? null, tick: by[o.id]?.tick ?? null, sentAt } : o));
      // A market order can fill before its ack is back: the live feed already said so.
      orders = orders.filter((o) => !(keys.has(o.id) && o.ref && doneIds.has(`${account}|${o.ref}`)));
      beacon('ok', list.length > 1 ? 'placedLinked' : 'placed', { order: orderName(list[0]), n: list.length - 1 });
      readAccount(account);
    } catch (e) {
      orders = orders.filter((o) => !keys.has(o.id));
      if (e.code === 'margin_missing' && e.details) marginAsk = { list, coords: e.details, message: e.message };
      else if (e.code === 'duplicate_order' && !confirmDuplicate) {
        dupAsk = { list, message: e.message };
        dupOpen = true;
      } else {
        // A halt the chart did not know of yet (a loss limit just reached): the banner shows it.
        if (e.code === 'trading_halted') readRisk();
        error = e.message;
        beacon('err', 'placeFailed', { reason: e.message });
      }
    }
  }

  // ── An order like one sent seconds ago ──
  // The duplicate limit holds it back until the user says to send it; it goes again under new
  // ids, since the account already took the refused ones.
  let dupAsk = $state(null);
  let dupOpen = $state(false);
  function resendDuplicate() {
    const ask = dupAsk;
    dupAsk = null;
    if (ask) addOrders(reissue(ask.list), null, { confirmDuplicate: true });
  }

  // ── Halts and the kill switch ──
  // The halts in force, read while an account trades (a halt ends on its own at the hour, the
  // day or the week), after the kill switch, a lift, and an order refused for a halt.
  let risk = $state(null);
  async function readRisk() {
    try {
      risk = await brokersApi.risk();
    } catch {
      /* no banner rather than a wrong one */
    }
  }
  const RISK_MS = 60000;
  $effect(() => {
    if (!tradeIds) return;
    return poll(readRisk, RISK_MS);
  });
  const chartHalts = $derived(tradeAccounts.length ? haltsOn(risk, accountOf(activePane)?.id ?? null) : []);
  async function liftHalt(id) {
    try {
      await brokersApi.liftHalt(id);
      beacon('ok', 'lifted', {});
    } catch (e) {
      error = e.message;
      beacon('err', 'liftFailed', { reason: e.message });
    }
    readRisk();
  }
  let killOpen = $state(false);
  let killing = $state(false);
  /** Every working order on every trading account cancelled at its broker, then every account
   *  halted. Each account says how it went; one the broker could not be reached for is halted
   *  too. */
  async function killSwitch() {
    killing = true;
    try {
      const r = await brokersApi.killSwitch();
      const rows = Object.values(r.accounts ?? {});
      const parts = rows.map((a) =>
        a.error != null
          ? $t('histviz.trade.killFailed', { name: a.name ?? '', error: a.error })
          : $t('histviz.trade.killCancelled', { name: a.name ?? '', n: a.cancelled ?? 0 })
      );
      toast.push(rows.some((a) => a.error != null) ? 'warn' : 'ok', $t('histviz.trade.killed', { list: parts.join('; ') }), 10000);
      for (const a of tradeAccounts) readAccount(a.id);
    } catch (e) {
      error = e.message;
      beacon('err', 'killFailed', { reason: e.message });
    } finally {
      killing = false;
      readRisk();
    }
  }

  // ── A paper future with no margin stated ──
  // The order is held back and the two ways out are offered: state the margin per contract in
  // the instrument settings, or count the contract at its full value. Either one sends it again.
  let marginAsk = $state(null);
  let marginModalOpen = $state(false);
  let marginBusy = $state(false);
  function resendMargin() {
    const ask = marginAsk;
    marginAsk = null;
    // New ids: the account already took the refused ones.
    if (ask) addOrders(reissue(ask.list));
  }
  async function useFullValue() {
    if (!marginAsk) return;
    marginBusy = true;
    try {
      const c = marginAsk.coords;
      const r = await instrumentMeta.info(c);
      await instrumentMeta.put({ ...(r?.meta ?? {}), provider: c.provider, asset_type: c.asset_type, ticker: c.ticker, margin_notional: true });
      resendMargin();
    } catch (e) {
      error = e.message;
    } finally {
      marginBusy = false;
    }
  }

  /** Set one order's fields, leaving the rest of the list as it is now. */
  const patchOrder = (id, patch) => (orders = orders.map((o) => (o.id === id ? { ...o, ...patch } : o)));

  /** A dragged line: the broker moves a working order, and the line goes back if it refuses. */
  async function moveLine({ id, level, price }) {
    const o = orders.find((x) => x.id === id);
    if (!o) return;
    if (!o.ref) {
      orders = moveOrder(orders, id, level, price);
      return;
    }
    if (o.status !== 'working') return;
    const was = o[level];
    orders = moveOrder(orders, id, level, price);
    patchOrder(id, { status: 'sending' });
    try {
      const r = await brokersApi.moveOrder(o.account, o.ref, { [level]: dec(price) }, BOOK_MODULE);
      const ended = doneWhileMoving.delete(`${o.account}|${o.ref}`);
      // A broker that changes it as a new order (Binance USDⓈ-M stops) names the new one.
      if (ended && !r?.replaced_by) {
        orders = orders.filter((x) => x.id !== id);
        beacon('warn', 'cancelled', { order: orderName(o) });
      } else {
        patchOrder(id, { status: 'working', ...(r?.replaced_by ? { ref: r.replaced_by.id } : {}) });
        beacon('ok', 'moved', { order: orderName(o) });
      }
    } catch (e) {
      // Refused: the order stands as it was, unless the stream said it ended meanwhile.
      if (doneWhileMoving.delete(`${o.account}|${o.ref}`)) orders = orders.filter((x) => x.id !== id);
      patchOrder(id, { status: 'working', [level]: was });
      if (e.code === 'trading_halted') readRisk();
      error = e.message;
      beacon('err', 'moveFailed', { reason: e.message });
    }
  }

  /** Cancel at the broker first; the line goes once it confirmed. Its exits go with it, at the
   *  broker as here. */
  async function cancelLine(id) {
    const o = orders.find((x) => x.id === id);
    if (!o) return;
    if (!o.ref) {
      orders = cancelOrder(orders, id);
      return;
    }
    if (o.status !== 'working') return;
    patchOrder(id, { status: 'sending' });
    cancelledHere.add(`${o.account}|${o.ref}`);
    try {
      await brokersApi.cancelOrder(o.account, o.ref, BOOK_MODULE);
      orders = cancelOrder(orders, id);
      beacon('ok', 'cancelled', { order: orderName(o) });
      refreshBooks(o.account);
    } catch (e) {
      cancelledHere.delete(`${o.account}|${o.ref}`);
      patchOrder(id, { status: 'working' });
      error = e.message;
      beacon('err', 'cancelFailed', { reason: e.message });
    }
  }

  /** Every order in the list: working entries at the broker (their exits follow), drafts here. */
  async function cancelAll() {
    for (const o of orders.filter((x) => !x.parent || !orders.some((p) => p.id === x.parent))) await cancelLine(o.id);
  }

  // ── Following the trading accounts ──
  // Every account with trading on is followed while the page is open: one book read
  // (positions and working orders, unpaced at the broker) per account every few seconds. It
  // draws each account's positions and orders on its charts, feeds the panel's Positions and
  // Orders tabs, and drops from the list what the broker stopped listing (filled, or
  // cancelled in TWS). A hidden page skips its turns. An account the live stream follows
  // (below) is read every RECONCILE_MS only: its changes arrive as they happen.
  const FOLLOW_MS = 4000;
  const RECONCILE_MS = 30000;
  /** accountId -> { book, at, error }: the last read of each followed account. */
  let accountBooks = $state({});
  const tradeAccounts = $derived(bookAccounts.filter((a) => (a.modules ?? []).includes('trading')));
  let followTimer = null;
  let following = false;
  async function follow() {
    clearTimeout(followTimer);
    followTimer = null;
    if (!following && !document.hidden) {
      following = true;
      try {
        const ids = new Set([
          ...tradeAccounts.map((a) => a.id),
          ...orders.filter((o) => o.ref && o.status === 'working').map((o) => o.account)
        ]);
        for (const account of ids) {
          if (liveAccounts.has(account) && Date.now() - (accountBooks[account]?.at ?? 0) < RECONCILE_MS) continue;
          await readAccount(account);
        }
      } finally {
        following = false;
      }
    }
    followTimer = setTimeout(follow, FOLLOW_MS);
  }
  onMount(() => {
    follow();
    return () => clearTimeout(followTimer);
  });

  // ── The trading accounts followed live ──
  // One stream for every trading account (`/api/brokers/events`): each order and position
  // change the broker pushes lands on the book when it happens (accountEvents.js), instead of
  // at the next read. A broker that pushes nothing is left to the read above. Plain
  // collections on purpose: routing state, nothing renders them.
  /** Accounts the stream follows right now. */
  const liveAccounts = new Set();
  /** `account|order id` of orders the feed said stopped working, for an ack still on its way. */
  const doneIds = new Set();
  /** account -> [{ at, ev }]: the last few seconds of pushes, replayed over a read that was
   *  already on its way when they came, so an older read never undoes a newer push. */
  const recentEvents = {};
  /** account -> timer: one full read shortly after a position moved, for the figures the
   *  feed does not carry (mark, entry, result). */
  const settleTimers = {};
  const RECENT_MS = 10000;
  const SETTLE_MS = 1000;
  const tradeIds = $derived(tradeAccounts.map((a) => a.id).sort().join(','));
  $effect(() => {
    const ids = tradeIds ? tradeIds.split(',') : [];
    if (!ids.length) return;
    const s = liveStream({
      url: () => brokersApi.eventsUrl(ids, BOOK_MODULE),
      events: {
        // (Re)connected: what changed while the stream was down is read once.
        ready: (d) => {
          for (const id of d?.live ?? []) {
            liveAccounts.add(id);
            readAccount(id);
          }
        },
        account: onAccountEvent,
        off: (d) => liveAccounts.delete(d?.account)
      },
      onstate: (st) => {
        if (st !== 'open') liveAccounts.clear();
      }
    });
    return () => {
      s.close();
      liveAccounts.clear();
    };
  });

  /** One push from a followed account, applied wherever the page keeps that account. */
  function onAccountEvent(ev) {
    const account = ev?.account;
    if (!account) return;
    if (ev.kind === 'resync') {
      readAccount(account);
      return;
    }
    const now = Date.now();
    recentEvents[account] = [...(recentEvents[account] ?? []).filter((r) => now - r.at < RECENT_MS), { at: now, ev }];
    if (ev.kind === 'order' && ev.key) {
      // The line this page drew takes the broker's id as soon as the broker names it.
      orders = orders.map((o) => (o.id === ev.key && o.account === account && !o.ref ? { ...o, status: 'working', ref: ev.order.id, sentAt: now } : o));
    } else if (ev.kind === 'filled') {
      const o = brokerOrder(account, ev.id);
      if (o && Number(ev.filled_qty) > 0 && Number(ev.filled_qty) < Number(o.qty)) beacon('ok', 'partFilled', { order: orderName(o), qty: ev.filled_qty });
    } else if (ev.kind === 'done' && orders.some((o) => o.account === account && String(o.ref) === String(ev.id) && o.status === 'sending')) {
      // Being moved: the old half of a change made as a new order, or a real end the move's
      // answer will show. Not dropped, not announced, until that answer.
      doneWhileMoving.add(`${account}|${ev.id}`);
    } else if (ev.kind === 'done') {
      doneIds.add(`${account}|${ev.id}`);
      if (doneIds.size > 200) doneIds.delete(doneIds.values().next().value);
      // A refusal after the ack (a paper order the account cannot afford when it fills).
      if (ev.reason && ev.status !== 'filled') error = ev.reason;
      const o = brokerOrder(account, ev.id);
      const name = o ? orderName(o) : String(ev.id);
      if (ev.status === 'filled') beacon('ok', 'filled', { order: name });
      else if (ev.status === 'cancelled' && !cancelledHere.delete(`${account}|${ev.id}`)) beacon('warn', 'cancelled', { order: name });
      else if (ev.status === 'rejected') beacon('err', 'rejected', { order: name, reason: ev.reason ?? '' });
      else if (ev.status === 'expired') beacon('warn', 'expired', { order: name });
      orders = orders.filter((o) => !(o.account === account && String(o.ref) === String(ev.id)));
    } else if (ev.kind === 'position') {
      clearTimeout(settleTimers[account]);
      settleTimers[account] = setTimeout(() => readAccount(account), SETTLE_MS);
    }
    const cur = accountBooks[account];
    if (!cur?.book) return;
    const book = applyEvent(cur.book, ev);
    if (book === cur.book) return;
    accountBooks[account] = { ...cur, book };
    for (const p of ws.panes) if (p.broker === account) books[p.id] = { book, at: cur.at, busy: false, error: '' };
  }

  // Live marks of what the trading accounts hold, every second: the book's own prices are
  // the broker's last valuation (minutes old at IB), and the Positions tab should move like
  // the chart's position labels do. Only the held instruments are asked for; the broker
  // closes a line nobody asks about.
  const MARK_MS = 1000;
  /** accountId -> { [native_id]: price } */
  let accountMarks = $state({});
  let markTimer = null;
  async function readMarks() {
    clearTimeout(markTimer);
    if (!document.hidden) {
      await Promise.all(
        tradeAccounts.map(async (a) => {
          const ids = [...new Set((accountBooks[a.id]?.book?.positions ?? []).map((p) => p.native_id).filter(Boolean))];
          if (!ids.length) {
            if (accountMarks[a.id]) delete accountMarks[a.id];
            return;
          }
          try {
            accountMarks[a.id] = await brokersApi.marks(a.id, ids, BOOK_MODULE);
          } catch {
            // The book read reports the account's error; a mark is a refinement.
            delete accountMarks[a.id];
          }
        })
      );
    }
    markTimer = setTimeout(readMarks, MARK_MS);
  }
  onMount(() => {
    readMarks();
    return () => clearTimeout(markTimer);
  });

  /** One read of an account's book, handed to every chart trading through it. */
  async function readAccount(account) {
    const asked = Date.now();
    try {
      let book = await brokersApi.book(account, BOOK_MODULE);
      for (const r of recentEvents[account] ?? []) if (r.at >= asked) book = applyEvent(book, r.ev);
      const at = Date.now();
      accountBooks[account] = { book, at, error: '' };
      for (const p of ws.panes) if (p.broker === account) books[p.id] = { book, at, busy: false, error: '' };
      // A side that failed to read must not pass for "everything filled".
      if ((book.warnings ?? []).length) return;
      const listed = new Set((book.orders ?? []).map((o) => String(o.id)));
      const left = gone(orders, account, listed, asked);
      const drop = new Set(left.map((o) => o.id));
      // A broker read on a cycle only says the order stopped working, not why.
      for (const o of left) if (!liveAccounts.has(account)) beacon('warn', 'gone', { order: orderName(o) });
      if (drop.size) orders = orders.filter((o) => !drop.has(o.id));
    } catch (e) {
      accountBooks[account] = { ...accountBooks[account], error: e.message };
    }
  }
  const refreshBooks = (account) => readAccount(account);

  // ── Changing a working order ──
  // Double-click on its line, or its pencil in the Orders tab: the ticket opens on it, filled
  // with its values, and submitting sends the change in place at the broker.
  let ticketEdit = $state(null);
  /** Order types as the ticket names them: IB's codes, and paper's own words (already the
   * ticket's). Another venue's word leaves it unknown. */
  const TYPE_OF = {
    LMT: 'limit', STP: 'stop', 'STP LMT': 'stop_limit', MKT: 'market', MIT: 'mit', LIT: 'lit',
    LIMIT: 'limit', LIMIT_MAKER: 'limit', STOP_LOSS: 'stop', STOP_LOSS_LIMIT: 'stop_limit', TAKE_PROFIT: 'mit', TAKE_PROFIT_LIMIT: 'lit',
    MARKET: 'market', STOP_MARKET: 'stop', STOP: 'stop_limit', TAKE_PROFIT_MARKET: 'mit',
    limit: 'limit', stop: 'stop', stop_limit: 'stop_limit', market: 'market', mit: 'mit', lit: 'lit'
  };

  /** Counts every edit opened, never reset: the ticket loads an order when this changes. */
  let editSeq = 0;
  function openEdit(e) {
    editSeq += 1;
    ticketEdit = { ...e, n: editSeq };
    ticketUi.open = true;
    ticketUi.tab = 'ticket';
    ticketUi.expanded = true;
  }

  /** An order this page placed and still follows. */
  function editOrder(localId) {
    const o = orders.find((x) => x.id === localId);
    if (!o?.ref || o.status !== 'working') return;
    openEdit({ account: o.account, ref: String(o.ref), local: o.id, symbol: o.symbol, side: o.side, type: o.type, qty: o.qty, limit: o.limit, stop: o.stop, tif: o.tif ?? null, expires: o.expires ?? null });
  }

  /** An order the broker lists, placed here or elsewhere (TWS). */
  function editBrokerOrder(account, id) {
    const mine = orders.find((o) => o.account === account && String(o.ref) === String(id));
    if (mine) return editOrder(mine.id);
    const row = (accountBooks[account]?.book?.orders ?? []).find((o) => String(o.id) === String(id));
    // Binance spells a market-if-touched and a limit-if-touched alike (TAKE_PROFIT) on its
    // two markets: the limit price tells them apart.
    const named = TYPE_OF[row?.order_type];
    const type = named === 'mit' && Number(row?.limit_price) > 0 ? 'lit' : named;
    if (!row || !type) {
      error = $t('histviz.trade.err.cannotEdit');
      return;
    }
    const rest = Math.max(0, (Number(row.qty) || 0) - (Number(row.filled_qty) || 0));
    openEdit({ account, ref: String(id), local: null, symbol: row.symbol, side: row.side, type, qty: rest || row.qty, limit: row.limit_price, stop: row.stop_price, tif: null, expires: null });
  }

  /** Send the change; the order's line follows once the broker echoed it. */
  async function amendOrder(e, change) {
    const body = { ...change };
    for (const k of ['qty', 'limit', 'stop']) if (k in body) body[k] = dec(body[k]);
    if (e.local) patchOrder(e.local, { status: 'sending' });
    try {
      const r = await brokersApi.moveOrder(e.account, e.ref, body, BOOK_MODULE);
      if (e.local) patchOrder(e.local, { ...change, status: 'working', ...(r?.replaced_by ? { ref: r.replaced_by.id } : {}) });
      ticketEdit = null;
      const o = brokerOrder(e.account, e.ref);
      beacon('ok', 'changed', { order: o ? orderName(o) : String(e.ref) });
    } catch (err) {
      if (e.local) patchOrder(e.local, { status: 'working' });
      error = err.message;
      beacon('err', 'changeFailed', { reason: err.message });
    }
    readAccount(e.account);
  }

  // ── Closing a position ──
  // From its ✕ on a chart or in the Positions tab: a market order for what is held, on the
  // other side, sent on the instrument id the broker reported the position with. Asked
  // first: a close is an order at whatever the market is.
  let closeAsk = $state(null); // { account, position }
  let closeOpen = $state(false);
  function askClose(account, position) {
    if (!account || !position?.nativeId) return;
    closeAsk = { account, position };
    closeOpen = true;
  }
  async function closePosition() {
    const c = closeAsk;
    if (!c) return;
    try {
      await brokersApi.closePosition(
        c.account,
        { native_id: String(c.position.nativeId), account: c.position.account ?? '', key: `close-${Date.now().toString(36)}` },
        BOOK_MODULE
      );
      beacon('ok', 'closeSent', { symbol: c.position.symbol || String(c.position.nativeId) });
      await cancelExitsOf(c.account, c.position);
    } catch (e) {
      error = e.message;
      beacon('err', 'closeFailed', { reason: e.message });
    }
    readAccount(c.account);
  }

  /** A closed position's exits have nothing left to close: the working orders on its
   *  instrument, on the closing side, that the broker links to an entry or flags
   *  reduce-only. Each goes the way a cancel from the panel does. */
  async function cancelExitsOf(account, position) {
    const closing = position.side === 'short' ? 'buy' : 'sell';
    const exits = (accountBooks[account]?.book?.orders ?? []).filter(
      (o) => o.symbol === position.symbol && o.side === closing && (o.parent_id || o.reduce_only) && (!position.account || !o.account || o.account === position.account)
    );
    for (const o of exits) await cancelBrokerOrder(account, String(o.id));
    if (exits.length) beacon('ok', 'exitsCancelled', { n: exits.length, symbol: position.symbol });
  }

  /** Protect a position with exits: the ticket opens on its chart with a take profit and a
   *  stop loss on the other side, one cancelling the other. A position no chart on screen
   *  shows is said so, never traded on another instrument. */
  function protectPosition(account, position) {
    const pane = shown.find((p) => {
      if (p.broker !== account || !p.coords?.ticker) return false;
      const theirs = brokerSymbols[`${p.broker}|${instrumentKeyOf(p.coords)}`];
      return (theirs ?? p.coords.ticker) === position.symbol;
    });
    if (!pane) {
      error = $t('histviz.trade.err.protectNoChart', { symbol: position.symbol });
      beacon('err', 'protectFailed', { reason: error });
      return;
    }
    activate(pane.id);
    openTicketAt({ protect: { symbol: position.symbol, side: position.side, qty: position.qty } });
  }

  /** A working order listed by the broker, cancelled from the panel: through its line when
   *  this page placed it, straight at the broker otherwise (an order placed in TWS). */
  async function cancelBrokerOrder(account, id) {
    const mine = orders.find((o) => o.account === account && String(o.ref) === String(id));
    if (mine) return cancelLine(mine.id);
    const o = brokerOrder(account, id);
    cancelledHere.add(`${account}|${id}`);
    try {
      await brokersApi.cancelOrder(account, id, BOOK_MODULE);
      beacon('ok', 'cancelled', { order: o ? orderName(o) : String(id) });
    } catch (e) {
      cancelledHere.delete(`${account}|${id}`);
      error = e.message;
      beacon('err', 'cancelFailed', { reason: e.message });
    }
    readAccount(account);
  }

  /** Orders the broker never received, sent again as one ticket or dropped, from the panel. */
  async function resendUnsent(account, ids) {
    try {
      await brokersApi.resendUnsent(account, ids, BOOK_MODULE);
      beacon('ok', 'resent', { n: ids.length });
    } catch (e) {
      error = e.message;
      beacon('err', 'placeFailed', { reason: e.message });
    }
    readAccount(account);
  }
  async function dismissUnsent(account, ids) {
    try {
      await brokersApi.dismissUnsent(account, ids, BOOK_MODULE);
    } catch (e) {
      error = e.message;
    }
    readAccount(account);
  }

  /** account|instrument -> how the account spells the chart's instrument (`MNQZ6` for
   *  `MNQ.20261218@CME` at IB), asked of the broker once: the broker writes a dated contract
   *  its own way, and only its answer says which book rows are this chart's. */
  let brokerSymbols = $state({});
  /** account|instrument -> the size step the account's orders on it must be a multiple of. */
  let brokerSteps = $state({});
  /** account|instrument -> { leverage, marginMode } the account set on it at the venue, read only. */
  let brokerLevers = $state({});
  $effect(() => {
    for (const p of ws.panes) {
      const c = p.coords;
      if (!p.broker || !c?.ticker || !accountOf(p)) continue;
      const k = `${p.broker}|${instrumentKeyOf(c)}`;
      if (k in untrack(() => brokerSymbols)) continue;
      brokerSymbols[k] = null;
      brokersApi
        .symbolFor(p.broker, c.ticker, c.asset_type)
        .then((r) => {
          brokerSymbols[k] = r.symbol;
          brokerSteps[k] = r.step;
          brokerLevers[k] = { leverage: r.leverage, marginMode: r.marginMode };
        })
        .catch(() => {});
    }
  });

  /** A synced book as the chart reads it: rows the broker writes under its own spelling of
   *  the chart's instrument carry the chart's ticker, and the orders this page already draws
   *  as its own lines are left out, so a working order is one line, the one that drags. */
  function bookOf(p) {
    const book = books[p.id]?.book;
    if (!book) return null;
    const theirs = brokerSymbols[`${p.broker}|${instrumentKeyOf(p.coords)}`];
    const rename = (r) => (theirs && r.symbol === theirs ? { ...r, symbol: p.coords.ticker } : r);
    const mine = new Set(orders.filter((o) => o.ref && o.account === p.broker && !trailsInBook(o, book)).map((o) => String(o.ref)));
    return {
      ...book,
      positions: (book.positions ?? []).map(rename),
      orders: (book.orders ?? []).filter((o) => !mine.has(String(o.id))).map(rename)
    };
  }

  /** A working trailing stop is drawn from the broker's book, where it trails now, not from
   *  the price it was sent with. */
  const trailsInBook = (o, book) => o.type === 'trailing_stop' && !!o.ref && (book?.orders ?? []).some((r) => String(r.id) === String(o.ref));

  /** The orders a pane draws as its own lines: those the book draws instead are left out. */
  function ordersOf(p) {
    const book = books[p.id]?.book;
    return orders.filter((o) => !(o.account === p.broker && trailsInBook(o, book)));
  }

  /** The `+` menu's "open in ticket": the ticket shows, on this price. */
  function openTicketAt(p) {
    ticketUi.open = true;
    ticketPrefill = { ...p, n: (ticketPrefill?.n ?? 0) + 1 };
  }

  /** Live handles the panes register, so the shared stream and the rail can reach them.
   *  A plain Map on purpose: it is a routing table, not state anything renders. */
  const panes = new Map();

  const shown = $derived(visiblePanes(ws));
  const activePane = $derived(ws.panes.find((p) => p.id === ws.active) ?? shown[0] ?? null);
  const activeCoords = $derived(activePane?.coords ?? null);
  const ticketTarget = $derived(
    activeCoords?.ticker
      ? {
          symbol: activeCoords.ticker,
          instrument: instrumentKeyOf(activeCoords),
          connector: activePane?.connector_id ?? null,
          last: markets[activePane.id]?.last ?? null,
          digits: markets[activePane.id]?.digits ?? 2,
          bid: markets[activePane.id]?.bid ?? null,
          ask: markets[activePane.id]?.ask ?? null,
          tick: markets[activePane.id]?.tick ?? null,
          pointValue: markets[activePane.id]?.pointValue ?? null,
          qtyStep: brokerSteps[`${activePane.broker}|${instrumentKeyOf(activeCoords)}`] ?? null,
          leverage: brokerLevers[`${activePane.broker}|${instrumentKeyOf(activeCoords)}`]?.leverage ?? null,
          marginMode: brokerLevers[`${activePane.broker}|${instrumentKeyOf(activeCoords)}`]?.marginMode ?? null,
          kind: markets[activePane.id]?.kind ?? activeCoords.asset_type ?? null
        }
      : null
  );
  const activeBook = $derived(books[activePane?.id] ?? null);
  /** account|symbol (as the broker spells it) -> the latest price a chart on screen shows,
   *  so the Positions tab marks those positions live, as the chart's own labels do. */
  const livePrices = $derived.by(() => {
    const out = {};
    for (const p of shown) {
      const last = markets[p.id]?.last;
      if (!p.broker || !p.coords?.ticker || !(last > 0)) continue;
      const theirs = brokerSymbols[`${p.broker}|${instrumentKeyOf(p.coords)}`];
      out[`${p.broker}|${theirs ?? p.coords.ticker}`] = last;
    }
    return out;
  });
  /** account|order id -> where a trailing stop trails now, as a chart on screen works it out
   *  (Binance reports only the level it was placed at). */
  const trailPrices = $derived.by(() => {
    const out = {};
    for (const p of shown) for (const [id, price] of Object.entries(markets[p.id]?.trails ?? {})) if (p.broker) out[`${p.broker}|${id}`] = price;
    return out;
  });
  /** Accounts connected to the charts on screen, each counted once. */
  const connectedCount = $derived(new Set(shown.map((p) => p.broker).filter((id) => id && bookAccounts.some((a) => a.id === id))).size);
  /** Whether any order is working or on its way: the broker's, or this page's. */
  const ordersPending = $derived(
    tradeAccounts.some((a) => (accountBooks[a.id]?.book?.orders?.length ?? 0) > 0) || orders.some((o) => o.status === 'sending' || o.status === 'working')
  );

  // ── Catalog + workspace load ──
  let booted = false;
  $effect(() => {
    if (booted) return;
    booted = true;
    boot();
  });

  async function boot() {
    // Not awaited: the charts load meanwhile, and a chart saved with an account unlocks
    // trading once the list is in.
    loadBookAccounts();
    try {
      datasets = await histvizApi.datasets();
    } catch (e) {
      error = e.message;
    }
    try {
      providers = await histvizApi.providers();
    } catch {
      /* the capability matrix is only used for fallbacks */
    }
    try {
      connectors = await connectorsApi.list('histviz');
    } catch {
      /* the live account picker falls back to the server's own default */
    }
    try {
      workspaces = await histvizApi.workspaces();
    } catch (e) {
      error = e.message;
    }
    const wanted = localStorage.getItem(LAST_KEY);
    const row = workspaces.find((w) => w.id === wanted) ?? workspaces[0] ?? null;
    if (row) await openRow(row);
    else {
      ws.panes[0].coords = lastSingleChart();
      baseline = savedKey($state.snapshot(ws));
    }
    if (!ws.active) ws.active = ws.panes[0]?.id ?? null;
    // The panes render and register themselves before anything can be asked of them.
    await tick();

    // A deep link from the download module opens that dataset in the active pane; one from an
    // alert (its notification, the global alert list) opens the alert's instrument.
    const id = $page.url.searchParams.get('dataset');
    const d = id ? datasets.find((x) => x.id === id) : null;
    const alertId = $page.url.searchParams.get('alert');
    if (alertId) {
      openedAlert = alertId;
      loadAll(ws.active);
      await openAlert(alertId);
    } else if (d) {
      const c = connectors.find((x) => x.provider === d.provider);
      // An imported series has no connector: what it can show is what the catalog stores.
      const stored = datasets
        .filter((x) => x.provider === d.provider && x.asset_type === d.asset_type && x.ticker === d.ticker)
        .map((x) => x.timeframe);
      openInActive({
        connector_id: c?.id ?? null,
        provider: d.provider,
        asset_type: d.asset_type,
        ticker: d.ticker,
        timeframe: d.timeframe,
        timeframes: c?.timeframes ?? stored,
        stream: (c?.stream_asset_types ?? []).includes(d.asset_type),
        stream_timeframes: c?.stream_timeframes ?? [],
        stream_note: c?.stream_note ?? ''
      });
    } else {
      loadAll();
    }
    loaded = true;
  }

  /** Open every pane that has an instrument, in one round trip. Four panes used to be four
   *  requests fired at once, which a shared connector answers by rate-limiting the lot. */
  async function loadAll(skip = null) {
    const list = shown.filter((p) => p.coords?.ticker && p.id !== skip);
    if (!list.length) return;
    for (const p of list) panes.get(p.id)?.setLoading?.(true);
    try {
      const results = await histvizApi.seriesBatch(
        list.map((p) => ({ ...$state.snapshot(p.coords), bars: SLICE_BARS }))
      );
      list.forEach((p, i) => panes.get(p.id)?.apply?.(results[i]));
    } catch (e) {
      error = e.message;
      for (const p of list) panes.get(p.id)?.setLoading?.(false);
    }
    syncStream();
  }

  // ── Workspace persistence ──
  // Saved on request only (the Save button, Ctrl/Cmd+S), like a trading desk's layouts: an
  // edit changes the workspace on screen, and `dirty` says it differs from the last save.
  let loaded = $state(false);
  let saving = $state(false);
  /** `savedKey` of the workspace as last saved (or opened). */
  let baseline = $state('');
  const dirty = $derived(loaded && savedKey(ws) !== baseline);

  /** The stored row a body becomes, so the picker reopens a workspace as it was saved. */
  const rowOf = (id, body) => ({ id, name: body.name, grid_rows: body.rows, grid_cols: body.cols, panes: body.panes, settings: body.settings });

  /** Put a stored workspace on screen. One saved before workspaces held their charts takes
   *  its studies and drawings from the instrument layouts first. */
  async function openRow(row) {
    const next = row ? fromRow(row) : newWorkspace();
    if (row && next.legacy) await migrateLegacy(next, histvizApi.layoutOf, loadDrawings);
    if (!next.active) next.active = next.panes[0]?.id ?? null;
    ws = next;
    baseline = savedKey(next);
  }

  async function saveWorkspace() {
    if (saving) return false;
    saving = true;
    try {
      const snap = $state.snapshot(ws);
      const body = toBody(snap);
      if (ws.id) {
        await histvizApi.saveWorkspace(ws.id, body);
        workspaces = workspaces.map((w) => (w.id === ws.id ? { ...w, ...rowOf(ws.id, body) } : w));
      } else {
        const row = await histvizApi.createWorkspace(body);
        ws.id = row.id;
        workspaces = [...workspaces, { ...row, ...rowOf(row.id, body) }];
        localStorage.setItem(LAST_KEY, row.id);
      }
      baseline = savedKey(snap);
      return true;
    } catch (e) {
      error = e.message;
      return false;
    } finally {
      saving = false;
    }
  }

  /** Save what is on screen as a new workspace, which becomes the open one; the workspace it
   *  came from keeps its last save. */
  let saveAsOpen = $state(false);
  async function saveAs(name) {
    const snap = $state.snapshot(ws);
    snap.name = name;
    try {
      const body = toBody(snap);
      const row = await histvizApi.createWorkspace(body);
      workspaces = [...workspaces, { ...row, ...rowOf(row.id, body) }];
      ws.id = row.id;
      ws.name = name;
      localStorage.setItem(LAST_KEY, row.id);
      baseline = savedKey($state.snapshot(ws));
    } catch (e) {
      error = e.message;
    }
  }

  /** Back to the last save. */
  async function revert() {
    const row = workspaces.find((w) => w.id === ws.id);
    panes.clear();
    closeStream();
    maximized = null;
    await openRow(row ?? null);
    await tick();
    loadAll();
  }

  // ── Unsaved changes ──
  // Leaving the workspace (another one, a new one, another page) asks first when it has
  // unsaved changes: save, discard, or stay.
  let unsavedOpen = $state(false);
  let unsavedGo = null;
  function guard(go) {
    if (!dirty) return go();
    unsavedGo = go;
    unsavedOpen = true;
  }
  async function unsavedPick(choice) {
    const go = unsavedGo;
    unsavedGo = null;
    if (choice === 'save' && !(await saveWorkspace())) return;
    if (choice === 'save' || choice === 'discard') {
      baseline = savedKey($state.snapshot(ws)); // nothing left to ask about on the way out
      go?.();
    }
  }
  beforeNavigate((nav) => {
    if (!dirty || nav.type === 'leave' || !nav.to) return;
    nav.cancel();
    const url = nav.to.url;
    guard(() => goto(url));
  });
  function onBeforeUnload(e) {
    if (!dirty) return;
    e.preventDefault();
    e.returnValue = '';
  }

  async function switchWorkspace(id) {
    if (id === ws.id) return;
    const row = workspaces.find((w) => w.id === id);
    if (!row) return;
    closeStream();
    panes.clear();
    maximized = null;
    await openRow(row);
    localStorage.setItem(LAST_KEY, row.id ?? '');
    // The panes remount and register themselves; load once they have.
    await tick();
    loadAll();
  }

  async function addWorkspace() {
    const fresh = newWorkspace($t('histviz.workspace.untitled'));
    fresh.panes[0].coords = activeCoords ? { ...$state.snapshot(activeCoords) } : null;
    try {
      const row = await histvizApi.createWorkspace(toBody(fresh));
      workspaces = [...workspaces, row];
      panes.clear();
      closeStream();
      await openRow(row);
      localStorage.setItem(LAST_KEY, row.id);
      await tick();
      loadAll();
    } catch (e) {
      error = e.message;
    }
  }

  /** Delete a workspace. Another one than the open one leaves the screen alone; the open one
   *  hands the screen to the first that remains. */
  async function removeWorkspace(id = ws.id) {
    if (!id) return;
    if (id !== ws.id) {
      try {
        await histvizApi.deleteWorkspace(id);
        workspaces = workspaces.filter((w) => w.id !== id);
      } catch (e) {
        error = e.message;
      }
      return;
    }
    try {
      await histvizApi.deleteWorkspace(id);
      workspaces = workspaces.filter((w) => w.id !== id);
      panes.clear();
      closeStream();
      const next = workspaces[0];
      await openRow(next ?? null);
      localStorage.setItem(LAST_KEY, next?.id ?? '');
      await tick();
      loadAll();
    } catch (e) {
      error = e.message;
    }
  }

  // ── Chart templates ──
  // Kept in the global chart settings, so every workspace offers the same list; applying one
  // changes the charts, which the workspace saves like any other edit.
  const templates = $derived(templatesIn(settings));
  function saveTemplate(pane, name) {
    const tpl = templateOf($state.snapshot(pane), name);
    const old = templates.find((x) => x.name === tpl.name);
    if (old) tpl.id = old.id;
    settings = { ...settings, chartTemplates: [...templates.filter((x) => x.name !== tpl.name), tpl] };
  }
  const deleteTemplate = (id) =>
    (settings = { ...settings, chartTemplates: templates.filter((x) => x.id !== id) });

  // A row deleted from the library: the server took its studies off the saved workspaces and
  // templates, so drop them from what this page holds too (the charts on screen drop their
  // own). The saved key loses them as well, so the deletion is not an unsaved edit.
  $effect(() =>
    onIndicatorRemoved((id) => {
      const strip = (panes) => panes?.map((p) => ({ ...p, instances: withoutIndicator(p.instances ?? [], id) }));
      for (const p of ws.panes) p.instances = withoutIndicator(p.instances ?? [], id);
      workspaces = workspaces.map((w) => ({ ...w, panes: strip(w.panes) }));
      if (baseline) {
        const body = JSON.parse(baseline);
        body.panes = strip(body.panes);
        baseline = JSON.stringify(body);
      }
      if (templates.some((x) => withoutIndicator(x.instances ?? [], id).length !== (x.instances ?? []).length))
        settings = { ...settings, chartTemplates: templates.map((x) => ({ ...x, instances: withoutIndicator(x.instances ?? [], id) })) };
    })
  );

  /** "Apply to charts": the template and the charts ticked (every chart on screen to start). */
  let applying = $state(null);
  let applyIds = $state([]);
  function openApplyTo(tpl) {
    applying = tpl;
    applyIds = shown.map((p) => p.id);
  }
  function applyTemplateTo() {
    for (const id of applyIds) panes.get(id)?.useTemplate?.(applying);
    applying = null;
  }
  const paneLabel = (p) =>
    p.coords?.ticker ? `${p.coords.ticker} · ${p.coords.timeframe ?? ''}` : $t('histviz.templates.emptyChart');

  // ── Grid shape ──
  function setShape(rows, cols) {
    const before = new Set(shown.map((p) => p.id));
    reshape(ws, rows, cols);
    maximized = null;
    // A pane the grid no longer shows stops streaming (its instrument leaves the shared
    // connection); the panes still on screen carry on through the swap, see openStream.
    syncStream();
    // Panes that just appeared inherit the active instrument, so they draw something rather
    // than asking the user to pick what they are already looking at.
    tick().then(() => {
      const fresh = visiblePanes(ws).filter((p) => !before.has(p.id) && p.coords?.ticker);
      if (fresh.length) loadPanes(fresh);
    });
  }

  async function loadPanes(list) {
    for (const p of list) panes.get(p.id)?.setLoading?.(true);
    try {
      const results = await histvizApi.seriesBatch(
        list.map((p) => ({ ...$state.snapshot(p.coords), bars: SLICE_BARS }))
      );
      list.forEach((p, i) => panes.get(p.id)?.apply?.(results[i]));
    } catch (e) {
      error = e.message;
      for (const p of list) panes.get(p.id)?.setLoading?.(false);
    }
    syncStream();
  }

  function closePane(id) {
    if (ws.panes.length <= 1) return;
    ws.panes = ws.panes.filter((p) => p.id !== id);
    panes.delete(id);
    const cells = ws.rows * ws.cols;
    if (ws.panes.length < cells) ws.panes.push(newPane(activeCoords ? { ...activeCoords } : null));
    if (ws.active === id) ws.active = ws.panes[0]?.id ?? null;
    if (maximized === id) maximized = null;
    syncStream();
  }

  const activate = (id) => {
    if (ws.active === id) return;
    ws.active = id;
  };

  /** The instrument an alert watches, in the active pane, read through the alert's own
   *  connector when it still has one. */
  let openedAlert = null;
  async function openAlert(id) {
    try {
      const a = await histvizApi.alert(id);
      const c = connectors.find((x) => x.id === a.connector_id) ?? connectors.find((x) => x.provider === a.provider);
      openInActive({
        connector_id: c?.id ?? null,
        provider: a.provider,
        asset_type: a.asset_type,
        ticker: a.ticker,
        timeframe: a.timeframe,
        timeframes: c?.timeframes ?? [],
        stream: (c?.stream_asset_types ?? []).includes(a.asset_type),
        stream_timeframes: c?.stream_timeframes ?? [],
        stream_note: c?.stream_note ?? ''
      });
    } catch (e) {
      error = e.message;
    }
  }
  // Already on the page (the global alert list links here): follow the link in place.
  $effect(() => {
    const id = $page.url.searchParams.get('alert');
    if (!loaded || !id || id === openedAlert) return;
    openedAlert = id;
    untrack(() => openAlert(id));
  });

  function openInActive(coords) {
    const id = ws.active ?? shown[0]?.id;
    railRef?.remember?.(coords);
    panes.get(id)?.openCoords?.(coords);
    if (railOpen && matchMedia('(max-width: 767px)').matches) toggleRail();
  }

  /** A rail row that cannot be charted. The pane says so in place of its chart: the reason is
   *  a connector to fix, and the pane is where the user is already looking. */
  function failInActive(info) {
    panes.get(ws.active ?? shown[0]?.id)?.showBlocked?.(info);
  }

  // ── Splitters ──
  // A drag moves one boundary: the track before it grows by what the one after it loses, so
  // the grid never reflows as a whole and no pane can be collapsed out of reach.
  function dragTrack(e, axis, index) {
    e.preventDefault();
    const box = gridEl?.getBoundingClientRect();
    if (!box) return;
    // The charts share the axis minus the ticket when it is one of its tracks.
    const span = (axis === 'row' ? box.height : box.width) - (midAxis === axis ? midT : 0);
    const start = axis === 'row' ? e.clientY : e.clientX;
    const before = axis === 'row' ? ws.row_sizes.slice() : ws.col_sizes.slice();
    const move = (ev) => {
      const now = axis === 'row' ? ev.clientY : ev.clientX;
      const delta = (now - start) / span;
      const next = resizeTrack(before, index, delta);
      if (axis === 'row') ws.row_sizes = next;
      else ws.col_sizes = next;
    };
    const up = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
      };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
  }

  /** Where a splitter sits, as a percentage of the axis: the fractions before it over the
   *  total. Percentages rather than pixels so a window resize needs no recomputation. */
  function offsets(list) {
    const total = list.reduce((a, b) => a + b, 0) || 1;
    const out = [];
    let cum = 0;
    for (let i = 0; i < list.length - 1; i++) {
      cum += list[i];
      out.push((cum / total) * 100);
    }
    return out;
  }

  // ── Where the order ticket sits ──
  // It takes room instead of covering charts: at the bottom it raises them, at the top it
  // lowers them, on a side it moves them over. In the middle it slips between two rows
  // (horizontal) or two columns (vertical) as one more grid track, so it needs two of them:
  // on a grid without, or maximized, or on a phone, it falls back to the nearest edge.
  const NARROW = '(max-width: 767px)';
  let narrow = $state(matchMedia(NARROW).matches);
  $effect(() => {
    const mq = matchMedia(NARROW);
    const on = () => (narrow = mq.matches);
    mq.addEventListener('change', on);
    return () => mq.removeEventListener('change', on);
  });
  const midOk = $derived({
    'center-h': !maximized && !narrow && ws.rows >= 2,
    'center-v': !maximized && !narrow && ws.cols >= 2
  });
  const ticketDocks = $derived(TICKET_DOCKS.filter((d) => midOk[d] ?? true));
  const ticketDock = $derived(
    midOk[ticketUi.dock] === false ? (ticketUi.dock === 'center-h' ? 'bottom' : 'right') : ticketUi.dock
  );
  /** The axis the ticket is a track of, when it sits in the middle. */
  const midAxis = $derived(
    !ticketUi.open ? null : ticketDock === 'center-h' ? 'row' : ticketDock === 'center-v' ? 'col' : null
  );
  /** Tracks before the ticket: the middle of the grid. */
  const midK = $derived(midAxis === 'row' ? Math.floor(ws.rows / 2) : midAxis === 'col' ? Math.floor(ws.cols / 2) : 0);
  let midW = $state(0);
  let midH = $state(0);
  let gridGap = $state(0);
  $effect(() => {
    if (gridEl) gridGap = parseFloat(getComputedStyle(gridEl).rowGap) || 0;
  });
  /** Pixels the ticket takes out of its axis, gap included: the charts share the rest. */
  const midT = $derived(midAxis === 'row' ? midH + gridGap : midAxis === 'col' ? midW + gridGap : 0);

  function tracks(sizes, axis) {
    const fr = sizes.map((x) => `${x}fr`);
    if (midAxis === axis) fr.splice(midK, 0, 'auto');
    return fr.join(' ');
  }

  /** A cell's place once a track was inserted before it; the grid's own flow otherwise. */
  function cellPlace(i) {
    if (!midAxis || maximized) return '';
    const r = Math.floor(i / ws.cols);
    const c = i % ws.cols;
    const row = r + 1 + (midAxis === 'row' && r >= midK ? 1 : 0);
    const col = c + 1 + (midAxis === 'col' && c >= midK ? 1 : 0);
    return `grid-row:${row};grid-column:${col}`;
  }

  /** A splitter's offset along an axis the ticket is a track of: its share of what the charts
   *  have, pushed past the ticket when it sits after it. */
  function splitAt(pct, i, axis) {
    if (midAxis !== axis) return `${pct}%`;
    return `calc(${pct}% - ${(pct * midT) / 100}px + ${i >= midK ? midT : 0}px)`;
  }

  const gridStyle = $derived(
    maximized
      ? 'grid-template-rows:1fr;grid-template-columns:1fr'
      : `grid-template-rows:${tracks(ws.row_sizes, 'row')};` +
        `grid-template-columns:${tracks(ws.col_sizes, 'col')}`
  );

  // ── The shared live stream ──
  let es = null;
  let streamKey = '';
  let streamTimer = null;

  /** Rebuild the connection when the *set* of live instruments changes. Deliberately not an
   *  effect over pane state: `bars` moves on every tick, and a stream that reopened for that
   *  would spend a connection per candle. */
  function syncStream() {
    clearTimeout(streamTimer);
    streamTimer = setTimeout(openStream, 150);
  }

  function openStream() {
    const reqs = [];
    const ids = [];
    for (const p of visiblePanes(ws)) {
      const r = panes.get(p.id)?.streamRequest?.();
      if (r) {
        reqs.push(r);
        ids.push(p.id);
      }
    }
    const key = ids
      .map(
        (id, i) =>
          `${id}|${reqs[i].provider}|${reqs[i].ticker}|${reqs[i].timeframe}|${reqs[i].connector_id ?? ''}|${reqs[i].quotes ? 'q' : ''}`
      )
      .join('~');
    if (key === streamKey) return;
    streamKey = key;
    // Make before break: the new connection opens first and the old one closes only once it
    // is up, so a pane that stays live never goes dark. Each connection routes with its own
    // pane list (the indices differ between the two), and a bar both deliver is merged by
    // its timestamp. Whatever closes during the swap comes back as the new stream's catch-up
    // (`since` is each pane's newest bar), and the server keeps a dropped subscription
    // through a grace period, so the upstream feed of a remaining instrument is never torn down.
    const prev = es;
    es = null;
    if (!reqs.length) {
      prev?.close();
      return;
    }
    let retired = false;
    const retire = () => {
      if (retired) return;
      retired = true;
      prev?.close();
    };
    // A connection that never opens must not keep the old one forever.
    setTimeout(retire, 5000);
    const route = (fn) => (d) => panes.get(ids[d?.i ?? 0])?.[fn]?.(d);
    // Supervised (lib/live/stream.js): a dropped, refused or silent connection is reopened
    // with backoff, and each reopen re-reads every pane's newest bar so the server's catch-up
    // starts from what the chart holds.
    es = liveStream({
      url: () =>
        histvizApi.streamsUrl(ids.map((id, i) => panes.get(id)?.streamRequest?.() ?? reqs[i])),
      events: {
        snapshot: route('applyBar'),
        bar: route('applyBar'),
        status: route('applyStatus'),
        // Interactive Brokers rations live data per account, not per connection: the server
        // says how many of the account's market-data lines the open panes are holding.
        lines: route('applyLines'),
        // Best bid and ask, for the panes that asked (the chart's bid/ask setting).
        quote: route('applyQuote'),
        quote_status: route('applyQuoteStatus')
      },
      onstate: (state, info) => {
        if (state === 'open') retire();
        // The server's own status events take over again once the connection is back.
        if (state === 'reconnecting') {
          const message = $t('histviz.live.serverLost', {
            seconds: Math.round((info.retryInMs ?? 0) / 1000)
          });
          for (const id of ids) {
            panes.get(id)?.applyStatus?.({ state: 'retrying', code: 'transport', message });
          }
        }
      }
    });
  }

  function closeStream() {
    if (es) {
      es.close();
      es = null;
    }
    streamKey = '';
  }

  // ── Global chart settings ──
  $effect(() => {
    histvizApi
      .chartSettings()
      .then((raw) => {
        settings = normalizeSettings(raw);
      })
      .catch(() => {
        /* fall back to defaults */
      })
      .finally(() => {
        settingsLoaded = true;
      });
  });
  $effect(() => {
    const snap = { ...settings };
    if (!settingsLoaded) return;
    histvizApi.saveChartSettings(snap).catch((e) => (error = e.message));
  });

  // Fullscreen folds the app top bar away; leaving the page always brings it back.
  $effect(() => () => {
    chrome.hideTopbar = false;
  });

  function onKey(e) {
    if ((e.key === 's' || e.key === 'S') && (e.ctrlKey || e.metaKey) && !e.altKey) {
      e.preventDefault();
      if (e.shiftKey) saveAsOpen = true;
      else if (dirty || !ws.id) saveWorkspace();
      return;
    }
    if (e.key === 'Escape' && shapeOpen) {
      shapeOpen = false;
      return;
    }
    if (e.key === 'Escape' && maximized) {
      maximized = null;
      return;
    }
    if (e.key === 'Escape' && chrome.hideTopbar) {
      chrome.hideTopbar = false;
      return;
    }
    if ((e.key === 'z' || e.key === 'Z') && (e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey) {
      const el = e.target;
      const tag = el?.tagName;
      if (el?.isContentEditable || tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return;
      const api = panes.get(ws.active);
      if (!api) return;
      if (api.hasQuickFills?.()) api.quickUndo?.();
      else if (api.hasDrawings?.()) api.undoDrawing?.();
      else return;
      e.preventDefault();
    }
  }

  /** The picker's rows. The open workspace is labelled from `ws.name`, not from its stored
   *  row, so a rename reads back immediately instead of after the debounced save. */
  const wsOpts = $derived([
    ...(ws.id ? [] : [{ value: '', label: ws.name || $t('histviz.workspace.untitled') }]),
    ...workspaces.map((w) => ({ value: w.id, label: w.id === ws.id ? ws.name : w.name }))
  ]);

  async function startRename() {
    renaming = true;
    await tick();
    nameEl?.focus();
    nameEl?.select();
  }

  /** Drag the divider: the rail follows the pointer 1:1 and the width is stored on release,
   *  so a drag writes localStorage once instead of on every frame. */
  let railDrag = $state(null);
  function railDown(e) {
    if (e.button !== 0) return;
    railDrag = { x0: e.clientX, w0: railW };
    e.currentTarget.setPointerCapture?.(e.pointerId);
    e.preventDefault();
  }
  function railMove(e) {
    if (!railDrag) return;
    railW = clampRail(railDrag.w0 + (e.clientX - railDrag.x0));
  }
  function railUp(e) {
    if (!railDrag) return;
    railDrag = null;
    e.currentTarget.releasePointerCapture?.(e.pointerId);
    saveRailW();
  }
  function railKey(e) {
    const step = e.shiftKey ? 32 : 8;
    if (e.key === 'ArrowLeft') railW = clampRail(railW - step);
    else if (e.key === 'ArrowRight') railW = clampRail(railW + step);
    else return;
    e.preventDefault();
    saveRailW();
  }
  function saveRailW() {
    try {
      localStorage.setItem(RAIL_KEY, String(railW));
    } catch {
      /* non-fatal */
    }
  }

  function toggleRail() {
    railOpen = !railOpen;
    try {
      localStorage.setItem('otw.histviz.railOpen', railOpen ? '1' : '0');
    } catch {
      /* non-fatal */
    }
  }

  /** Accounts this module may read, loaded with the page: a chart saved with an account has
   *  to know at once whether it can be traded. */
  async function loadBookAccounts() {
    bookError = '';
    try {
      bookAccounts = await brokersApi.list(BOOK_MODULE);
    } catch (e) {
      bookError = e.message;
    }
  }

  function toggleBook() {
    bookOpen = !bookOpen;
    if (bookOpen) loadBookAccounts();
  }

  async function syncBook() {
    const p = activePane;
    if (!p?.broker) return;
    const id = p.id;
    books[id] = { ...books[id], busy: true, error: '' };
    try {
      const book = await brokersApi.book(p.broker, BOOK_MODULE);
      books[id] = { book, at: Date.now(), busy: false, error: '' };
    } catch (e) {
      books[id] = { ...books[id], busy: false, error: e.message };
    }
  }

  /** Another account means another book: the chart's levels are dropped until it is synced,
   *  rather than left showing the previous account's. */
  function pickBookAccount(accountId) {
    if (!activePane) return;
    activePane.broker = accountId || null;
    clearBook();
  }

  function clearBook() {
    if (activePane) delete books[activePane.id];
  }

  function toggleQuick() {
    quickOn = !quickOn;
    try {
      localStorage.setItem(QUICK_KEY, quickOn ? '1' : '0');
    } catch {
      /* non-fatal */
    }
  }

  /** A pane publishes from an effect, so the map is read outside the reactive graph: reading
   *  it inside that effect would make the effect depend on what it writes, and the flush
   *  would never settle. */
  const onQuickSession = (id, session) => {
    untrack(() => {
      if (quickSessions[id] === session) return;
      quickSessions = { ...quickSessions, [id]: session };
    });
  };

  /** Every chart's trades in one session, in the order they closed. The pane is carried on
   *  the row so deleting a trade reaches the chart that owns its fills. */
  const quickAgg = $derived.by(() => {
    const trades = [];
    const opens = [];
    for (const p of shown) {
      const q = quickSessions[p.id];
      if (!q) continue;
      const label = `${q.symbol} · ${q.timeframe}`;
      for (const tr of q.trades)
        trades.push({ ...tr, key: `${p.id}:${tr.id}`, paneId: p.id, symbol: q.symbol, timeframe: q.timeframe });
      if (q.open) opens.push({ ...q.open, paneId: p.id, symbol: q.symbol, label, last: q.last });
    }
    trades.sort((a, b) => a.closeTs - b.closeTs);
    return { trades, opens };
  });

  /** The charts a click can land on: the panes showing an instrument. */
  const quickCharts = $derived(
    shown
      .filter((p) => p.coords?.ticker)
      .map((p) => ({ value: p.id, label: `${p.coords.ticker} · ${p.coords.timeframe ?? ''}`.trim() }))
  );
  const quickTarget = $derived(quickCharts.some((c) => c.value === ws.active) ? ws.active : '');
  const quickSizing = $derived(quickSessions[quickTarget] ?? null);

  function quickReset() {
    for (const p of shown) panes.get(p.id)?.quickReset?.();
  }

  const register = (id, api) => {
    panes.set(id, api);
    // A pane that came up outside the workspace's opening batch (a restored layout, a
    // maximize) loads its own instrument: the batch it was not part of will never reach it,
    // and a chart showing its symbol next to "choose an instrument" is a dead pane.
    setTimeout(() => {
      if (panes.get(id) === api && api.needsLoad?.()) api.load?.();
    }, 0);
  };
  const unregister = (id) => panes.delete(id);

  // ── Link groups ──
  // A pane's colour is its group. A group shares the instrument and the instant it is
  // pointing at; the timeframe is deliberately never shared, because three panes on one
  // symbol at 1m, 1h and 1d is the reason to link them at all. The visible span only travels
  // between panes on the same timeframe, since the same two dates are a screenful on one and
  // two candles on the other.
  function linked(fromId, kind, payload) {
    if (kind === 'restream') {
      syncStream();
      return;
    }
    const from = ws.panes.find((p) => p.id === fromId);
    const group = from?.link;
    if (!group) return;
    for (const p of visiblePanes(ws)) {
      if (p.id === fromId || p.link !== group) continue;
      const api = panes.get(p.id);
      if (!api) continue;
      if (kind === 'symbol') api.followCoords?.(payload);
      else if (kind === 'crosshair') api.showCrosshair?.(payload);
      else if (kind === 'range') api.setTimeRange?.(payload, payload.timeframe);
    }
    if (kind === 'symbol') syncStream();
  }
</script>

<RequireModule module="histviz">
  <!-- Button descriptions show at once rather than after the browser's own delay. -->
  <div class="page" class:full={chrome.hideTopbar} use:quickTitles>
    <!-- ── Workspace bar ──
         One fixed-height row that never wraps and never changes its set of controls. Left:
         the rail toggle, the workspace (picker or rename field, same slot), then the active
         chart's symbol, studies and data. Right edge: the rest of the chart's controls, the
         quick backtest, the order ticket, the book, the layout. Anything that only makes
         sense sometimes is disabled, never removed, so no control ever moves under the
         pointer. -->
    <header class="top">
      <!-- Only this half scrolls when the window is narrow. The settings popover lives
           outside it: a scroll container clips an absolutely positioned menu. -->
      <div class="top-scroll">
      <button
        class="tbtn"
        aria-pressed={railOpen}
        title={railOpen ? $t('histviz.rail.collapse') : $t('histviz.rail.show')}
        aria-label={railOpen ? $t('histviz.rail.collapse') : $t('histviz.rail.show')}
        onclick={toggleRail}
      >
        <Icon name={railOpen ? 'chevrons-left' : 'list'} size={13} />
      </button>

      <span class="rule" aria-hidden="true"></span>

      <div class="ws-slot">
        {#if renaming}
          <input
            class="ws-name"
            bind:this={nameEl}
            bind:value={ws.name}
            onblur={() => (renaming = false)}
            onkeydown={(e) => {
              if (e.key === 'Enter' || e.key === 'Escape') renaming = false;
            }}
            aria-label={$t('histviz.workspace.name')}
          />
        {:else}
          <div class="ws-pick" use:clickOutside={() => (wsMenuOpen = false)}>
            <button
              bind:this={wsBtn}
              class="ws-btn"
              class:open={wsMenuOpen}
              aria-haspopup="menu"
              aria-expanded={wsMenuOpen}
              title={$t('histviz.workspace.switch')}
              onclick={toggleWsMenu}
            >
              <span class="ws-label">{ws.name || $t('histviz.workspace.untitled')}</span>
              <Icon name={wsMenuOpen ? 'chevron-up' : 'chevron-down'} size={12} />
            </button>
            {#if wsMenuOpen}
              <div
                class="ws-menu"
                role="menu"
                style={wsMenuAt
                  ? `top:${wsMenuAt.top}px;left:${wsMenuAt.left}px;width:${wsMenuAt.width}px`
                  : ''}
              >
                <button
                  class="ws-add"
                  role="menuitem"
                  onclick={() => {
                    wsMenuOpen = false;
                    guard(addWorkspace);
                  }}
                >
                  <Icon name="plus" size={13} />
                  <span>{$t('histviz.workspace.add')}</span>
                </button>
                <button
                  class="ws-add"
                  role="menuitem"
                  onclick={() => {
                    wsMenuOpen = false;
                    saveAsOpen = true;
                  }}
                >
                  <Icon name="copy" size={13} />
                  <span>{$t('histviz.workspace.saveAs')}</span>
                </button>
                <button
                  class="ws-add"
                  role="menuitem"
                  disabled={!dirty || !ws.id}
                  onclick={() => {
                    wsMenuOpen = false;
                    revertOpen = true;
                  }}
                >
                  <Icon name="rotate-ccw" size={13} />
                  <span>{$t('histviz.workspace.revert')}</span>
                </button>
                {#each wsOpts as o (o.value)}
                  <div class="ws-row" class:current={(ws.id ?? '') === o.value}>
                    <button
                      class="ws-go"
                      role="menuitem"
                      onclick={() => {
                        wsMenuOpen = false;
                        guard(() => switchWorkspace(o.value));
                      }}>{o.label}</button
                    >
                    <button
                      class="ws-act"
                      title={$t('histviz.workspace.rename')}
                      aria-label={$t('histviz.workspace.rename')}
                      onclick={() => renameFromMenu(o.value)}
                    >
                      <Icon name="pencil" size={12} />
                    </button>
                    <button
                      class="ws-act danger"
                      title={$t('histviz.workspace.remove')}
                      aria-label={$t('histviz.workspace.remove')}
                      disabled={!o.value}
                      onclick={() => askDelete(o.value, o.label)}
                    >
                      <Icon name="trash-2" size={12} />
                    </button>
                  </div>
                {/each}
              </div>
            {/if}
          </div>
        {/if}
      </div>

      <!-- Save: the workspace changes only on screen until this is pressed (or Ctrl/Cmd+S).
           The dot says there is something to save. -->
      <button
        class="tbtn wide save"
        class:dirty
        disabled={saving || (!dirty && !!ws.id)}
        title={dirty ? $t('histviz.workspace.saveHint') : $t('histviz.workspace.saved')}
        aria-label={$t('histviz.workspace.save')}
        onclick={saveWorkspace}
      >
        <Icon name={dirty || !ws.id ? 'save' : 'check'} size={13} />
        <span class="tbtn-lbl">{dirty || !ws.id ? $t('histviz.workspace.save') : $t('histviz.workspace.saved')}</span>
        {#if dirty}<span class="unsaved" aria-hidden="true"></span>{/if}
      </button>
      </div>

      <span class="rule" aria-hidden="true"></span>

      <!-- The active chart's own controls (symbol, timeframe, studies, alerts, live...). One
           bar for every chart, TradingView style: the pane with the focus renders into it. -->
      <div class="pane-dock" bind:this={barDock}></div>

      <span class="rule" aria-hidden="true"></span>

      <!-- Quick backtest: one session over every chart on screen, so the switch belongs to
           the workspace rather than to a pane. -->
      <button
        class="tbtn"
        class:on={quickOn}
        aria-pressed={quickOn}
        title={$t('histviz.page.quickBacktestHint')}
        aria-label={$t('histviz.page.quickBacktest')}
        onclick={toggleQuick}
      >
        <Icon name="zap" size={13} />
      </button>
      <!-- The active chart's hand-off to the backtest module, next to the quick one. -->
      <div class="act-dock" bind:this={actDock}></div>

      <!-- Broker book: the active chart's account. Outside the scroller
           for the same reason as the settings sheet: a scroll container clips a popover. -->
      <div class="book-wrap" use:clickOutside={() => (bookOpen = false)}>
        <button
          class="tbtn wide"
          class:on={bookOpen}
          aria-expanded={bookOpen}
          aria-haspopup="true"
          title={$t('histviz.book.hint')}
          onclick={toggleBook}
        >
          <Icon name="briefcase" size={13} />
          <span class="tbtn-lbl">{$t('histviz.book.connect')}</span>
          {#if connectedCount}<span class="badge">{connectedCount}</span>{/if}
        </button>
        {#if bookOpen}
          <BookPanel
            accounts={bookAccounts}
            accountId={activePane?.broker ?? ''}
            coords={activeCoords}
            lock={tradeLockOf(activePane)}
            onpickaccount={pickBookAccount}
            book={activeBook?.book ?? null}
            syncedAt={activeBook?.at ?? 0}
            busy={!!activeBook?.busy}
            error={activeBook?.error || bookError}
            tickers={activeCoords?.ticker ? [activeCoords.ticker] : []}
            onsync={syncBook}
            onclear={clearBook}
            onaccounts={loadBookAccounts}
            onclose={() => (bookOpen = false)}
          />
        {/if}
      </div>

      <!-- Order ticket: one for the workspace, working on the active chart. -->
      <button
        class="tbtn wide"
        class:on={ticketUi.open}
        aria-pressed={ticketUi.open}
        title={$t('histviz.trade.ticketHint')}
        onclick={() => (ticketUi.open = !ticketUi.open)}
      >
        <Icon name="receipt" size={13} />
        <span class="tbtn-lbl">{$t('histviz.trade.title')}</span>
        {#if ordersPending}<span class="dot" title={$t('histviz.trade.pending')}></span>{/if}
      </button>

      <!-- The active chart's remaining controls: the Buy/Sell bar, templates, style, link,
           maximize, close. -->
      <div class="act-dock" bind:this={ctlDock}></div>

      <!-- Layout: a dropdown whose menu *is* the grid. Pointing at a cell says the shape
           better than a list of names ever did, and the cells light up on hover so the
           layout is read before it is applied. Outside the scroller: a scroll container
           clips an absolutely positioned menu. -->
      <div class="shape-wrap" use:clickOutside={() => (shapeOpen = false)}>
        <button
          class="tbtn wide"
          class:on={shapeOpen}
          aria-expanded={shapeOpen}
          aria-haspopup="true"
          title={$t('histviz.workspace.shape')}
          onclick={() => (shapeOpen = !shapeOpen)}
        >
          <Icon name="grid" size={13} />
          <span class="shape-lbl">{ws.rows}×{ws.cols}</span>
          <Icon name={shapeOpen ? 'chevron-up' : 'chevron-down'} size={12} />
        </button>
        {#if shapeOpen}
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <div
            class="shape-pop"
            role="group"
            aria-label={$t('histviz.workspace.shape')}
            onpointerleave={() => (shapeHover = null)}
          >
            {#each Array(MAX_ROWS) as _, r}
              <div class="shape-row">
                {#each Array(MAX_COLS) as _, c}
                  <button
                    class="cellbtn"
                    class:on={r < ws.rows && c < ws.cols}
                    class:pre={shapeHover && r <= shapeHover.r && c <= shapeHover.c}
                    aria-label={`${r + 1}x${c + 1}`}
                    title={`${r + 1}x${c + 1}`}
                    onpointerenter={() => (shapeHover = { r, c })}
                    onfocus={() => (shapeHover = { r, c })}
                    onblur={() => (shapeHover = null)}
                    onclick={() => {
                      setShape(r + 1, c + 1);
                      shapeOpen = false;
                    }}
                  ></button>
                {/each}
              </div>
            {/each}
            <p class="shape-cur">
              {shapeHover ? `${shapeHover.r + 1}×${shapeHover.c + 1}` : `${ws.rows}×${ws.cols}`}
            </p>
          </div>
        {/if}
      </div>

      <!-- Fullscreen: folds the app top bar (search, agent, account) away, keeps the rest. -->
      <button
        class="tbtn"
        class:on={chrome.hideTopbar}
        aria-pressed={chrome.hideTopbar}
        title={chrome.hideTopbar ? $t('histviz.page.exitFullscreenEsc') : $t('histviz.page.fullscreen')}
        aria-label={chrome.hideTopbar ? $t('histviz.page.exitFullscreenEsc') : $t('histviz.page.fullscreen')}
        onclick={() => (chrome.hideTopbar = !chrome.hideTopbar)}
      >
        <Icon name={chrome.hideTopbar ? 'minimize' : 'maximize'} size={13} />
      </button>

    </header>

    <nav class="mobile-pane-nav" aria-label={$t('histviz.page.title')}>
      <button class:current={railOpen} aria-pressed={railOpen} onclick={toggleRail}>{railOpen ? $t('histviz.rail.collapse') : $t('histviz.rail.show')}</button>
      {#each shown as p, i (p.id)}
        <button class:current={!railOpen && ws.active === p.id} aria-pressed={!railOpen && ws.active === p.id} onclick={() => { activate(p.id); if (railOpen) toggleRail(); }}>
          {p.coords?.ticker ?? `${$t('histviz.page.title')} ${i + 1}`}
        </button>
      {/each}
    </nav>

    <div class="body">
      {#if railOpen}
        <WatchRail
          bind:this={railRef}
          current={activeCoords}
          {datasets}
          {connectors}
          onpick={openInActive}
          onfail={failInActive}
          ondownload={() => goto('/histdata')}
          width={railW}
        />
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
        <div
          class="rail-grip"
          class:on={!!railDrag}
          role="separator"
          aria-orientation="vertical"
          aria-label={$t('histviz.rail.resize')}
          title={$t('histviz.rail.resize')}
          aria-valuenow={railW}
          aria-valuemin={RAIL_MIN}
          aria-valuemax={RAIL_MAX}
          tabindex="0"
          onpointerdown={railDown}
          onpointermove={railMove}
          onpointerup={railUp}
          onpointercancel={railUp}
          ondblclick={() => {
            railW = 198;
            saveRailW();
          }}
          onkeydown={railKey}
        ></div>
      {/if}

      <!-- The active chart's drawing tools, shared like the bar above. -->
      <div class="draw-dock" class:mobile-rail={railOpen} bind:this={railDock}></div>

      <div class="grid-wrap" class:mobile-rail={railOpen}>
        <!-- Errors float over the grid instead of pushing it down: a chart that resizes the
             moment a request fails loses the window the user was reading. -->
        {#if error}
          <div class="err-float err-row">
            <ErrorText {error} copyable compact />
            <Button variant="ghost" size="sm" icon="x" aria-label={$t('common.close')} title={$t('common.close')} onclick={() => (error = '')} />
          </div>
        {/if}
        {#if marginAsk}
          <div class="err-float margin-ask" role="alert">
            <Icon name="alert-triangle" size={13} />
            <span class="margin-msg">{marginAsk.message}</span>
            <span class="margin-actions">
              <Button variant="primary" size="sm" disabled={marginBusy} onclick={() => (marginModalOpen = true)}>{$t('histviz.trade.margin.set')}</Button>
              <Button size="sm" disabled={marginBusy} onclick={useFullValue}>{$t('histviz.trade.margin.notional')}</Button>
              <Button variant="ghost" size="sm" icon="x" aria-label={$t('common.cancel')} onclick={() => (marginAsk = null)} />
            </span>
          </div>
        {/if}
        <div class="stage dock-{ticketUi.open && !midAxis ? ticketDock : 'none'}">
        {#if ticketUi.open && !midAxis}
          <div class="edge-slot at-{ticketDock}">{@render ticketView()}</div>
        {/if}
        <div class="grid-box">
        <!-- Maximizing hides the other cells and gives the grid a single track; the pane
             itself is never unmounted. A second copy of it would come up with no bars, no
             zoom and no drawings, next to its own symbol in the bar. -->
        <div class="grid" bind:this={gridEl} style={gridStyle}>
          {#if midAxis}
            <div
              class="mid-slot"
              bind:clientWidth={midW}
              bind:clientHeight={midH}
              style={midAxis === 'row' ? `grid-row:${midK + 1};grid-column:1 / -1` : `grid-column:${midK + 1};grid-row:1 / -1`}
            >
              {@render ticketView()}
            </div>
          {/if}
          {#each shown as p, i (p.id)}
            <div class="cell" class:off={maximized && maximized !== p.id} class:mobile-inactive={ws.active !== p.id} style={cellPlace(i)}>
              <Pane
                bind:pane={ws.panes[i]}
                {settings}
                {datasets}
                {providers}
                {connectors}
                quick={quickOn}
                book={bookOf(p)}
                orders={ordersOf(p)}
                tradeAccount={tradeAccountOf(p)}
                tradeStep={brokerSteps[`${p.broker}|${instrumentKeyOf(p.coords)}`] ?? null}
                tradeLock={lockText(p)}
                ontrade={addOrders}
                onordermove={moveLine}
                onordercancel={cancelLine}
                onorderedit={editOrder}
                onbookorderedit={(id) => editBrokerOrder(p.broker, id)}
                onpositionclose={tradeAccountOf(p) ? (pos) => askClose(p.broker, pos) : null}
                onticket={openTicketAt}
                onmarket={(id, m) => (markets[id] = m)}
                active={ws.active === p.id}
                {barDock}
                {actDock}
                {rangeDock}
                {ctlDock}
                {railDock}
                maximized={maximized === p.id}
                closable={ws.panes.length > 1 && !maximized}
                onactivate={activate}
                onregister={register}
                onunregister={unregister}
                onquick={onQuickSession}
                onchanged={syncStream}
                bind:boards={ws.drawings}
                {templates}
                onsavetemplate={(name) => saveTemplate(p, name)}
                ondeletetemplate={deleteTemplate}
                onapplyto={openApplyTo}
                onmaximize={(id) => (maximized = maximized === id ? null : id)}
                onclose={closePane}
                onlinked={(kind, payload) => linked(p.id, kind, payload)}
                ondatasets={(rows) => (datasets = rows)}
                onsettings={(patch) => (settings = { ...settings, ...patch })}
                onapplysettings={(style) => {
                  // Every chart takes this style, and so does the next one opened.
                  for (const p of ws.panes) p.settings = { ...style };
                  settings = { ...settings, ...style };
                }}
              />
            </div>
          {/each}
        </div>
        <!-- Splitters ride over the gaps rather than in the flow: the grid stays a plain
             rows x columns, and a handle is just a percentage down the axis. -->
        {#if !maximized}
          <div class="splitters">
            {#each offsets(ws.row_sizes) as pct, i}
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <div
                class="split row"
                style={`top:${splitAt(pct, i, 'row')}`}
                onpointerdown={(e) => dragTrack(e, 'row', i)}
              ></div>
            {/each}
            {#each offsets(ws.col_sizes) as pct, i}
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <div
                class="split col"
                style={`left:${splitAt(pct, i, 'col')}`}
                onpointerdown={(e) => dragTrack(e, 'col', i)}
              ></div>
            {/each}
          </div>
        {/if}
        </div>
        </div>
      </div>
      {#snippet ticketView()}
        <OrderTicket
          target={ticketTarget}
          {livePrices}
          {trailPrices}
          {accountMarks}
          account={tradeAccountOf(activePane)}
          lock={lockText(activePane)}
          {orders}
          dock={ticketDock}
          docks={ticketDocks}
          onpickdock={(d) => (ticketUi.dock = d)}
          bind:expanded={ticketUi.expanded}
          bind:tab={ticketUi.tab}
          bind:journal={ticketUi.journal}
          bind:sizes={ticketUi.sizes}
          prefill={ticketPrefill}
          oncreate={(list) => addOrders(list)}
          oncancel={cancelLine}
          oncancelall={cancelAll}
          oncancelbroker={cancelBrokerOrder}
          onresendunsent={resendUnsent}
          ondismissunsent={dismissUnsent}
          oneditline={editOrder}
          oneditbroker={editBrokerOrder}
          oncloseposition={askClose}
          onprotectposition={protectPosition}
          edit={ticketEdit}
          onamend={amendOrder}
          onendedit={() => (ticketEdit = null)}
          {tradeAccounts}
          {accountBooks}
          bind:broker={ticketUi.broker}
          halts={chartHalts}
          onlifthalt={liftHalt}
          onkillswitch={() => (killOpen = true)}
          {killing}
          onclose={() => (ticketUi.open = false)}
        />
      {/snippet}
    </div>

    <!-- The active chart's range bar (1D, 1M, 1Y…): one fixed bar under every chart, like
         the bar above them. Fixed height whether or not the chart has an instrument. -->
    <footer class="range-dock" bind:this={rangeDock}></footer>

    <!-- The quick session spans the page, not a pane: fills come from every chart on screen
         and the numbers here are their sum. -->
    {#if quickOn}
      <QuickPanel
        trades={quickAgg.trades}
        opens={quickAgg.opens}
        charts={quickCharts}
        target={quickTarget}
        size={quickSizing?.size ?? 1}
        sizeMode={quickSizing?.mode ?? 'units'}
        contractSize={quickSizing?.contract ?? 1}
        bind:expanded={quickExpanded}
        bind:height={quickHeight}
        bind:tab={quickTab}
        onpicktarget={activate}
        onsizing={(patch) => panes.get(ws.active)?.setQuickSizing?.(patch)}
        onremove={(row) => panes.get(row.paneId)?.quickRemoveTrade?.(row)}
        onreset={quickReset}
        onundo={() => panes.get(ws.active)?.quickUndo?.()}
        onclose={toggleQuick}
      />
    {/if}
  </div>

  <ConfirmModal
    bind:open={confirmDelete}
    title={$t('histviz.workspace.remove')}
    message={$t('histviz.workspace.removeConfirm', { name: deleting?.name ?? ws.name })}
    confirmLabel={$t('common.delete')}
    cancelLabel={$t('common.cancel')}
    danger
    onconfirm={() => removeWorkspace(deleting?.id ?? ws.id)}
  />
</RequireModule>

<ChoiceModal
  bind:open={unsavedOpen}
  title={$t('histviz.workspace.unsavedTitle')}
  message={$t('histviz.workspace.unsavedMessage', { name: ws.name })}
  cancelLabel={$t('common.cancel')}
  choices={[
    { value: 'discard', label: $t('histviz.workspace.discard'), variant: 'ghost' },
    { value: 'save', label: $t('histviz.workspace.save'), variant: 'primary' }
  ]}
  onpick={unsavedPick}
  oncancel={() => (unsavedGo = null)}
/>

<ConfirmModal
  bind:open={closeOpen}
  title={$t('histviz.trade.closePosition')}
  message={closeAsk
    ? $t('histviz.trade.closeConfirm', {
        side: $t(`histviz.book.side.${closeAsk.position.side}`),
        qty: closeAsk.position.qty,
        symbol: closeAsk.position.symbol,
        account: tradeAccounts.find((a) => a.id === closeAsk.account)?.name ?? ''
      })
    : ''}
  confirmLabel={$t('histviz.trade.closePosition')}
  cancelLabel={$t('common.cancel')}
  danger
  onconfirm={closePosition}
/>

<ConfirmModal
  bind:open={killOpen}
  title={$t('brokers.risk.kill')}
  message={$t('brokers.risk.killMessage')}
  confirmLabel={$t('brokers.risk.killConfirm')}
  cancelLabel={$t('common.cancel')}
  danger
  onconfirm={killSwitch}
/>

<ConfirmModal
  bind:open={dupOpen}
  title={$t('histviz.trade.duplicateTitle')}
  message={dupAsk?.message ?? ''}
  confirmLabel={$t('histviz.trade.duplicateConfirm')}
  cancelLabel={$t('common.cancel')}
  onconfirm={resendDuplicate}
  oncancel={() => (dupAsk = null)}
/>

<ConfirmModal
  bind:open={revertOpen}
  title={$t('histviz.workspace.revert')}
  message={$t('histviz.workspace.revertConfirm', { name: ws.name })}
  confirmLabel={$t('histviz.workspace.discard')}
  cancelLabel={$t('common.cancel')}
  danger
  onconfirm={revert}
/>

<PromptModal
  bind:open={saveAsOpen}
  title={$t('histviz.workspace.saveAs')}
  fields={[{ key: 'name', label: $t('histviz.workspace.name'), value: `${ws.name} (2)`, required: true }]}
  confirmLabel={$t('histviz.workspace.save')}
  onconfirm={(v) => saveAs(String(v.name ?? '').trim() || ws.name)}
/>

<Modal open={!!applying} size="sm" title={$t('histviz.templates.applyTitle', { name: applying?.name ?? '' })} onclose={() => (applying = null)}>
  <div class="apply">
    <label class="apply-all">
      <input
        type="checkbox"
        checked={applyIds.length === shown.length}
        indeterminate={applyIds.length > 0 && applyIds.length < shown.length}
        onchange={(e) => (applyIds = e.currentTarget.checked ? shown.map((p) => p.id) : [])}
      />
      <span>{$t('histviz.templates.allCharts')}</span>
    </label>
    {#each shown as p (p.id)}
      <label class="apply-row">
        <input
          type="checkbox"
          checked={applyIds.includes(p.id)}
          onchange={(e) =>
            (applyIds = e.currentTarget.checked ? [...applyIds, p.id] : applyIds.filter((x) => x !== p.id))}
        />
        <span class="apply-name">{paneLabel(p)}</span>
        {#if p.id === ws.active}<span class="apply-tag">{$t('histviz.templates.activeChart')}</span>{/if}
      </label>
    {/each}
  </div>
  {#snippet footer()}
    <button class="ghost" onclick={() => (applying = null)}>{$t('common.cancel')}</button>
    <button class="primary" disabled={!applyIds.length} onclick={applyTemplateTo}>
      {$t('histviz.templates.applyN', { n: applyIds.length })}
    </button>
  {/snippet}
</Modal>

<svelte:window onkeydown={onKey} onbeforeunload={onBeforeUnload} />

{#if marginAsk}
  <InstrumentMetaModal bind:open={marginModalOpen} coords={marginAsk.coords} onsaved={resendMargin} />
{/if}

<style>
  /* The whole module area, measured by the shell rather than guessed from the viewport: a
     fixed offset left a strip under the charts. Folding the app top bar away grows the area,
     and the charts with it. */
  .page {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    height: 100%;
    min-height: 420px;
  }

  /* ── Workspace bar ──
     Fixed height, never wraps. Wrapping is a layout shift that costs the chart below a
     whole row of pixels, so the bar scrolls its own overflow instead. */
  .top {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex: none;
    height: 34px;
    padding: 0 var(--space-1);
    background: var(--surface);
    border: var(--hairline) solid var(--border);
    flex-wrap: nowrap;
  }
  .top-scroll {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    flex: 0 1 auto;
    min-width: 0;
    height: 100%;
    overflow-x: auto;
    overflow-y: hidden;
    scrollbar-width: none;
  }
  .top-scroll::-webkit-scrollbar {
    display: none;
  }
  .range-dock {
    display: flex;
    align-items: center;
    flex: none;
    height: 26px;
    min-width: 0;
    background: var(--surface);
    border: var(--hairline) solid var(--border);
  }
  .act-dock {
    display: flex;
    align-items: center;
    flex: none;
    height: 100%;
  }
  .act-dock:empty {
    display: none;
  }
  .pane-dock {
    display: flex;
    align-items: center;
    flex: 1 1 0;
    min-width: 0;
    height: 100%;
  }
  .draw-dock {
    display: flex;
    flex: none;
    min-height: 0;
    background: var(--surface);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .draw-dock:empty {
    display: none;
  }
  .draw-dock :global(.rail) {
    border-right: none;
  }
  /* Hairline group separator: the bar reads as three clusters, not eight buttons. */
  .rule {
    flex: none;
    width: var(--hairline);
    height: 18px;
    margin: 0 var(--space-1);
    background: var(--border);
  }

  /* One slot for the workspace, whether it is showing the picker or the rename field. */
  /* The token, not a :global override: the picker and the field are the same control at
     the same height, and nothing else in the slot has to know about it. */
  .ws-slot {
    --control-h: 26px;
    flex: none;
    width: 176px;
  }
  .ws-pick {
    width: 100%;
  }
  .ws-btn {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    width: 100%;
    height: 26px;
    padding: 0 var(--space-2);
    background: var(--surface);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    color: var(--text);
    font-size: var(--fs-body);
    cursor: pointer;
  }
  .ws-btn.open,
  .ws-btn:hover {
    border-color: var(--accent);
  }
  .ws-label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ws-menu {
    position: fixed;
    z-index: var(--z-dropdown);
    max-height: min(60vh, 420px);
    overflow-y: auto;
    padding: var(--space-1) 0;
    background: var(--surface);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-3);
  }
  .ws-add {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    padding: var(--space-2) var(--space-3);
    background: transparent;
    border: none;
    border-bottom: 1px solid var(--border);
    color: var(--accent);
    font-size: var(--fs-body);
    text-align: left;
    cursor: pointer;
  }
  .ws-add:hover:not(:disabled) {
    background: var(--surface-2);
  }
  .ws-add:disabled {
    color: var(--muted);
    opacity: 0.6;
    cursor: not-allowed;
  }
  .ws-row {
    display: flex;
    align-items: center;
    padding-right: var(--space-1);
  }
  .ws-row:hover {
    background: var(--surface-2);
  }
  .ws-go {
    flex: 1;
    min-width: 0;
    padding: var(--space-2) var(--space-3);
    background: transparent;
    border: none;
    color: var(--muted);
    font-size: var(--fs-body);
    text-align: left;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    cursor: pointer;
  }
  .ws-row.current .ws-go {
    color: var(--accent);
  }
  .ws-row:hover .ws-go {
    color: var(--text);
  }
  .ws-act {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: none;
    width: 26px;
    height: 26px;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
  }
  .ws-act:hover:not(:disabled) {
    color: var(--text);
    background: var(--surface);
  }
  .ws-act.danger:hover:not(:disabled) {
    color: var(--red);
  }
  .ws-act:disabled {
    opacity: 0.35;
    cursor: not-allowed;
  }
  .ws-name {
    width: 100%;
    height: 26px;
    padding: 0 var(--space-2);
    font-size: var(--fs-body);
  }

  .tbtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    flex: none;
    width: 26px;
    height: 26px;
    padding: 0;
    background: transparent;
    border: var(--hairline) solid transparent;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
    transition: color var(--dur-fast) var(--ease), background-color var(--dur-fast) var(--ease);
  }
  .tbtn.wide {
    width: auto;
    padding: 0 var(--space-2);
  }
  .tbtn-lbl {
    font-size: var(--fs-body);
    letter-spacing: 0.02em;
  }
  .tbtn:hover:not(:disabled) {
    background: var(--surface-2);
    color: var(--text);
  }
  .tbtn.on {
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 45%, var(--border));
    background: var(--surface-2);
  }
  .tbtn:disabled {
    opacity: 0.35;
    cursor: not-allowed;
  }
  /* Save: quiet once saved, accent with a dot while there are unsaved changes. */
  .tbtn.save {
    position: relative;
  }
  .tbtn.save:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .tbtn.save.dirty {
    color: var(--accent);
  }
  .unsaved {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--amber);
  }

  /* ── Apply a template to charts ── */
  .apply {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .apply-all,
  .apply-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2);
    border-radius: var(--radius);
    cursor: pointer;
  }
  .apply-all {
    border-bottom: 1px solid var(--border);
    border-radius: 0;
    font-weight: 600;
  }
  .apply-row:hover {
    background: var(--surface-2);
  }
  .apply-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .apply-tag {
    color: var(--muted);
    font-size: var(--fs-small, 11px);
  }

  /* ── Layout picker ──
     Twelve cells in a dropdown, hover previews the shape it would apply, the trigger spells
     out the current one. One click to open, one to apply. */
  .shape-wrap {
    position: relative;
    flex: none;
  }
  .shape-pop {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    z-index: var(--z-dropdown);
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: var(--space-2);
    background: var(--surface);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
  }
  .shape-row {
    display: flex;
    gap: 3px;
  }
  .cellbtn {
    width: 22px;
    height: 16px;
    padding: 0;
    background: transparent;
    border: var(--hairline) solid var(--border-control);
    border-radius: 0;
    cursor: pointer;
    transition: background-color var(--dur-fast) var(--ease);
  }
  .cellbtn.on {
    background: color-mix(in srgb, var(--accent) 45%, transparent);
    border-color: color-mix(in srgb, var(--accent) 55%, var(--border-control));
  }
  .cellbtn.pre {
    background: var(--accent);
    border-color: var(--accent);
  }
  /* Tabular width: 1x1 and 3x4 must occupy the same room or the trigger twitches. */
  .shape-lbl {
    display: inline-block;
    width: 26px;
    font-family: var(--mono);
    font-size: var(--fs-metric-label);
    letter-spacing: 0.04em;
    color: var(--dim);
    text-align: center;
  }
  .shape-cur {
    margin: 2px 0 0;
    font-family: var(--mono);
    font-size: var(--fs-metric-label);
    letter-spacing: 0.06em;
    color: var(--dim);
    text-align: center;
  }

  /* ── Broker book ── */
  .book-wrap {
    position: relative;
    flex: none;
  }
  /* How many levels the synced account has in total, so the button says the book is there
     without the panel having to be open. */
  /* An order is working: a mark, not a count. */
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--amber);
  }
  .badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    box-sizing: border-box;
    min-width: 15px;
    height: 15px;
    padding: 0 3px;
    border-radius: 999px;
    background: var(--accent);
    color: var(--accent-contrast);
    font-size: 9px;
    font-weight: var(--fw-medium);
    /* Tabular digits: a proportional "1" carries uneven side bearings and sits off-center. */
    font-variant-numeric: tabular-nums;
    line-height: 1;
  }

  .body {
    display: flex;
    gap: var(--space-2);
    flex: 1;
    min-height: 0;
  }
  /* The divider is a hairline until it is useful: a 7px hit area with a 1px line in it,
     lit on hover and while dragging. Double-click puts the rail back to its default. */
  .rail-grip {
    flex: none;
    width: 7px;
    /* The divider lives *in* the gap the flex row already leaves, so widening the hit area
       does not push the charts across. */
    margin: 0 calc(var(--space-2) / -2 - 3.5px);
    cursor: col-resize;
    background: transparent;
    touch-action: none;
  }
  .rail-grip::after {
    content: '';
    display: block;
    width: 1px;
    height: 100%;
    margin: 0 auto;
    background: transparent;
    transition: background 0.12s ease;
  }
  .rail-grip:hover::after,
  .rail-grip:focus-visible::after,
  .rail-grip.on::after {
    background: var(--accent);
  }
  .rail-grip:focus-visible {
    outline: none;
  }
  .grid-wrap {
    position: relative;
    flex: 1;
    min-width: 0;
    min-height: 0;
  }
  .margin-ask {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    font-size: var(--text-sm);
    color: var(--amber);
    border-color: color-mix(in srgb, var(--amber) 55%, var(--border));
    border-radius: var(--radius);
  }
  .margin-msg {
    flex: 1 1 240px;
    min-width: 0;
    color: var(--text);
    line-height: 1.4;
  }
  .margin-actions {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    flex: none;
  }
  .err-float {
    position: absolute;
    top: var(--space-2);
    left: var(--space-2);
    right: var(--space-2);
    z-index: var(--z-sticky);
    padding: var(--space-1) var(--space-2);
    background: var(--surface);
    border: var(--hairline) solid color-mix(in srgb, var(--red) 45%, var(--border));
  }
  .err-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .err-row > :global(.ui-error) {
    flex: 1;
    min-width: 0;
  }
  /* The charts and the ticket's edge slot share the space; the grid takes what is left. */
  .stage {
    display: flex;
    gap: var(--space-1);
    width: 100%;
    height: 100%;
    min-width: 0;
    min-height: 0;
  }
  .stage.dock-top,
  .stage.dock-bottom {
    flex-direction: column;
  }
  .grid-box {
    position: relative;
    flex: 1;
    min-width: 0;
    min-height: 0;
  }
  .edge-slot {
    display: flex;
    flex: none;
    min-width: 0;
    min-height: 0;
  }
  /* `at-*`, not the bare side names: `.top` is this page's bar. */
  .edge-slot.at-bottom,
  .edge-slot.at-right {
    order: 1;
  }
  .edge-slot.at-top,
  .edge-slot.at-bottom {
    max-height: 50%;
  }
  .mid-slot {
    display: flex;
    min-width: 0;
    min-height: 0;
    /* Over the splitters that cross it. */
    position: relative;
    z-index: 2;
  }
  .grid {
    display: grid;
    gap: var(--space-1);
    height: 100%;
    width: 100%;
  }
  .cell {
    display: flex;
    min-width: 0;
    min-height: 0;
  }
  .cell :global(.pane) {
    flex: 1;
    min-width: 0;
  }
  /* A maximized pane takes the only track the grid has left; the rest are hidden rather
     than unmounted, so nothing they hold is thrown away. */
  .cell.off {
    display: none;
  }
  .splitters {
    position: absolute;
    inset: 0;
    pointer-events: none;
  }
  .split {
    position: absolute;
    pointer-events: auto;
  }
  .split.row {
    left: 0;
    right: 0;
    height: 9px;
    transform: translateY(-50%);
    cursor: row-resize;
  }
  .split.col {
    top: 0;
    bottom: 0;
    width: 9px;
    transform: translateX(-50%);
    cursor: col-resize;
  }
  .split:hover {
    background: color-mix(in srgb, var(--accent) 35%, transparent);
  }
  .mobile-pane-nav { display: none; }
  /* Tablet and compact windows: the bar wraps once, so the active chart's symbol and
     timeframe keep a readable width instead of being squeezed behind the book button. */
  @media (min-width: 768px) and (max-width: 1099px) {
    .top { height: auto; min-height: 34px; flex-wrap: wrap; }
    .top-scroll { height: 32px; }
    .pane-dock { flex: 1 1 360px; height: 32px; }
  }
  @media (max-width: 767px) {
    /* The page scrolls; the chart area alone keeps about one screen of height, so the
       toolbars above it never squeeze the plot. The bottom room lets the last controls
       scroll clear of the floating shortcut buttons. */
    .page, .page.full { height: 100%; min-height: 0; overflow-y: auto; padding-bottom: calc(72px + env(safe-area-inset-bottom)); }
    .body { flex: none; height: max(320px, calc(100dvh - 120px)); }
    .top { height: auto; min-height: 44px; flex-wrap: wrap; padding: var(--space-1); }
    .top-scroll { flex: 1 1 100%; height: auto; min-height: 44px; flex-wrap: wrap; overflow: visible; }
    .top .rule { display: none; }
    .ws-slot { flex: 1 1 140px; width: auto; min-width: 0; }
    .top .tbtn { min-width: 44px; min-height: 44px; }
    .top .tbtn.wide { flex: 1 1 auto; }
    .book-wrap, .shape-wrap { flex: 1 1 auto; }
    .book-wrap .tbtn, .shape-wrap .tbtn { width: 100%; }
    .mobile-pane-nav { display: grid; grid-template-columns: repeat(auto-fill, minmax(88px, 1fr)); gap: var(--space-1); flex: none; }
    .mobile-pane-nav button { min-width: 0; min-height: 44px; padding: var(--space-2); overflow-wrap: anywhere; background: var(--surface); color: var(--text); border: 1px solid var(--border); }
    .mobile-pane-nav button.current { border-color: var(--accent); color: var(--accent); }
    .body { min-width: 0; }
    .body > :global(.rail) { width: 100% !important; flex: 1; min-width: 0; }
    .pane-dock { flex: 1 1 100%; height: auto; min-height: 44px; }
    .draw-dock.mobile-rail { display: none; }
    .rail-grip, .splitters { display: none; }
    .grid-wrap.mobile-rail { display: none; }
    .grid { grid-template-columns: minmax(0, 1fr) !important; grid-template-rows: minmax(0, 1fr) !important; }
    .cell.mobile-inactive { display: none; }
  }
</style>
