<script>
  // Backtest — one page, full width, no side pane.
  //
  // Top: a five-step wizard (Data → Strategy → Filters → Sizing & costs → Advanced). The steps
  // are tabs,
  // so they can be walked with Next/Previous or jumped between freely, and the run button is
  // live as soon as the minimum holds (one dataset) — the later steps refine, they don't gate.
  // A finished run replaces the wizard in that same area; "Edit strategy" goes back.
  //
  // Bottom (35%): the library dock — every run in History, every saved strategy in Strategies
  // (searched by name *and* tag). Strategies and custom indicators are created from here and
  // from the Strategy step: there is no separate expert mode.
  import Icon from '$lib/ui/Icon.svelte';
  import { copyLog } from '$lib/ui/copyLog.js';
  import { clickOutside } from '$lib/ui/clickOutside.js';
  import { onDestroy, tick, untrack } from 'svelte';
  import { goto } from '$app/navigation';
  import { page } from '$app/stores';
  import { t } from '$lib/i18n';
  import {
    backtestApi,
    defaultSettings,
    migrateSettings,
    normalizeSettings,
    embedIndicators,
    sizingNeedsStop,
    sideHasStop,
    toAnalysisTrades,
    fmtNum,
    buildReportMd,
    reportFilename,
    downloadText
  } from '$lib/modules/backtest/api.js';
  import { dataSummary, strategySummary, filtersSummary, sizingSummary, advancedSummary } from '$lib/modules/backtest/summary.js';
  import { indicatorLib } from '$lib/indicators/api.js';
  import DataStep from '$lib/modules/backtest/steps/DataStep.svelte';
  import StrategyStep from '$lib/modules/backtest/steps/StrategyStep.svelte';
  import FiltersStep from '$lib/modules/backtest/steps/FiltersStep.svelte';
  import SizingStep from '$lib/modules/backtest/steps/SizingStep.svelte';
  import AdvancedStep from '$lib/modules/backtest/steps/AdvancedStep.svelte';
  import { strategyInstances, tradeMarks, dcaMarks } from '$lib/modules/backtest/chart.js';
  import Chart from '$lib/modules/histviz/Chart.svelte';
  import IndicatorModal from '$lib/modules/histviz/IndicatorModal.svelte';
  import { histvizApi } from '$lib/modules/histviz/api.js';
  import {
    DEFAULT_SETTINGS as DEFAULT_CHART_SETTINGS,
    normalizeSettings as normalizeChartSettings
  } from '$lib/modules/histviz/settings.js';
  import StatsGrid from '$lib/modules/backtest/StatsGrid.svelte';
  import PerfTable from '$lib/modules/backtest/PerfTable.svelte';
  import TradesTable from '$lib/modules/backtest/TradesTable.svelte';
  import AssetBreakdown from '$lib/modules/backtest/AssetBreakdown.svelte';
  import DcaPanel from '$lib/modules/backtest/DcaPanel.svelte';
  import DcaHoldings from '$lib/modules/backtest/DcaHoldings.svelte';
  import DcaFills from '$lib/modules/backtest/DcaFills.svelte';
  import OosBlock from '$lib/modules/backtest/OosBlock.svelte';
  import TradeStats from '$lib/analysis/TradeStats.svelte';
  import TradeCurve from '$lib/analysis/TradeCurve.svelte';
  import { stats as closedTradeStats } from '$lib/analysis/trades.js';
  import HistoryPanel from '$lib/modules/backtest/HistoryPanel.svelte';
  import PaperPanel from '$lib/modules/backtest/PaperPanel.svelte';
  import PaperModal from '$lib/modules/backtest/PaperModal.svelte';
  import { paperApi, defaultSession } from '$lib/modules/backtest/paper.js';
  import StrategyLibrary from '$lib/modules/backtest/StrategyLibrary.svelte';
  import IndicatorLibraryModal from '$lib/modules/backtest/IndicatorLibraryModal.svelte';
  import SaveStrategyModal from '$lib/modules/backtest/SaveStrategyModal.svelte';
  import OptimizeModal from '$lib/modules/backtest/OptimizeModal.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import RequireModule from '$lib/modules/RequireModule.svelte';

  // ── Libraries ──
  let datasets = $state([]);
  let runs = $state([]);
  let strategies = $state([]);
  let indicators = $state([]);

  // ── The draft being edited ──
  let settings = $state(defaultSettings());
  let datasetIds = $state([]);
  let strategyId = $state(null); // provenance (null = unsaved draft)
  let strategyName = $state('');
  let strategyTags = $state([]);

  // "Unsaved changes" is a comparison, not a flag: the draft is dirty when it no longer matches
  // the snapshot taken the last time it was loaded or saved. A flag would need arming, and would
  // miss the very first edit of a fresh page.
  const draftKey = () => JSON.stringify({ s: settings, d: datasetIds, n: strategyName, t: strategyTags });
  let baseline = $state(JSON.stringify({ s: defaultSettings(), d: [], n: '', t: [] }));
  const dirty = $derived(draftKey() !== baseline);
  const markClean = () => (baseline = draftKey());

  // ── Run state ──
  let result = $state(null);
  // The headline of the run being replayed after a reload: what the screen shows while the
  // trades and the curves are being recomputed. Null at any other time.
  let restoring = $state(null);
  /** What the result screen reads: the real result, or the headline standing in for it. */
  const shown = $derived(result ?? restoring);
  let bars = $state(null);
  let chartAssetId = $state(null);
  let running = $state(false);
  let alignment = $state(null);
  let aligning = $state(false);
  let activeRunId = $state(null);

  // ── View state ──
  let step = $state('data');
  let showResult = $state(false);
  let chartHidden = $state(false);
  let tab = $state('summary'); // result detail tab

  // ── The result chart ──
  // It is the visualization module's chart, not a second one: same candles, same navigation,
  // same indicator engine. The run only feeds it (its strategy's indicators as chart
  // instances, and one marker per fill); the user is free to add or drop series from there
  // without touching the strategy.
  const CHART_H_KEY = 'otw.backtest.chartH';
  const readChartH = () => {
    const v = Number(localStorage.getItem(CHART_H_KEY));
    return Number.isFinite(v) && v >= 160 ? v : 380;
  };
  let chartSettings = $state({ ...DEFAULT_CHART_SETTINGS });
  let chartInstances = $state([]);
  let nextInstanceId = 1;
  let chartFull = $state(false);
  let chartH = $state(readChartH());
  let splitEl = $state(null);
  let indicatorOpen = $state(false);
  let editingInstance = $state(null);
  let pageEl = $state(null);
  let dockTab = $state('history');
  let paperSessions = $state([]);
  let paperUnseen = $state([]);
  let paperDraft = $state(null);
  let paperOpen = $state(false);
  let dockQuery = $state('');
  let error = $state('');

  // ── Optimizer ──
  // The grid is offered from the finished result, not from the builder: varying a parameter only
  // means something once one setting of it has been measured. A sweep left running elsewhere is
  // picked back up from the header, since the job outlives this page.
  let optimizeOpen = $state(false);
  let optJob = $state(null);
  /** Exactly what a run posts — the paths the optimizer varies have to index that object. */
  const optSettings = $derived(embedIndicators(normalizeSettings(settings), indicators));

  async function loadOptJob() {
    const jobs = await backtestApi.optimizeJobs().catch(() => []);
    optJob = jobs.find((j) => j.progress?.status === 'running') ?? null;
  }

  let reportMenu = $state(false);
  let saveRunOpen = $state(false);
  let saveRunName = $state('');
  let strategyModalOpen = $state(false);
  let strategyError = $state('');
  let indicatorsOpen = $state(false);

  const multi = $derived(datasetIds.length > 1);
  // Timeframe of the selected data (a run enforces one): the Filters step warns on it.
  const pickedTimeframe = $derived(datasets.find((d) => d.id === datasetIds[0])?.timeframe ?? '');
  // The selected datasets, in the order they were picked: what the DCA plan weights.
  const pickedAssets = $derived(datasetIds.map((id) => datasets.find((d) => d.id === id)).filter(Boolean));
  // The trades drawn on the chart are the charted asset's; a single-asset run carries no
  // ticker on its trades, so everything shows.
  const chartTicker = $derived(chartAssets.find((a) => a.id === chartAssetId)?.ticker ?? null);
  const chartTrades = $derived(
    chartAssets.length > 1 && chartTicker
      ? (result?.trades ?? []).filter((t) => t.ticker === chartTicker)
      : (result?.trades ?? [])
  );
  // A savings plan has no round trips: its markers are the fills themselves.
  const marks = $derived(
    result?.dca?.events
      ? dcaMarks(result.dca.events, chartAssets.length > 1 ? chartTicker : '')
      : tradeMarks(chartTrades)
  );
  // The result's trades in the shape $lib/analysis reads, plus their closed-trade statistics.
  // Realized only: the engine's own stats block carries the equity-based figures.
  const analysisTrades = $derived(toAnalysisTrades(result?.trades ?? []));
  const closedStats = $derived(closedTradeStats(analysisTrades));
  // Per-asset tab: '' reads the comparison table, a ticker reads that asset alone with the
  // same views the run gets. The engine's row carries what trades cannot say (exposure).
  let assetView = $state('');
  const assetRow = $derived((result?.per_asset ?? []).find((a) => a.ticker === assetView) ?? null);
  const assetTrades = $derived(
    assetView ? toAnalysisTrades((result?.trades ?? []).filter((t) => t.ticker === assetView)) : []
  );
  const assetStats = $derived(closedTradeStats(assetTrades));
  // A savings plan books a Trade only on a sell, so a pure accumulation has none: every
  // trade-derived view then reads the position at the last bar instead of an empty table.
  const dca = $derived(result?.dca ?? null);
  const dcaAssetRow = $derived((dca?.assets ?? []).find((a) => a.ticker === assetView) ?? null);
  const lastBarTs = $derived(result?.equity?.length ? result.equity[result.equity.length - 1].ts : '');
  // The tab counts what it actually lists: fills when there is no round trip to show.
  const tradeTabCount = $derived(
    !result ? shown.trades : result.trades.length || (dca ? (dca.events_total ?? 0) : 0)
  );
  const customIndicators = $derived(indicators.map((i) => ({ id: i.id, name: i.name })));
  const allTags = $derived([...new Set(strategies.flatMap((s) => s.tags ?? []))].sort());

  // Risk-based sizing without a stop can't size a position — the only other run blocker.
  const hasStop = $derived(
    (settings.mode !== 'short' && sideHasStop(settings.long)) ||
      (settings.mode !== 'long' && sideHasStop(settings.short))
  );
  // A savings plan sizes itself from its weights and tranches: the sizing block is dead
  // config there, so a risk-based mode left behind in it must not gate the run.
  const stopMissing = $derived(
    settings.kind !== 'dca' && sizingNeedsStop(settings.sizing) && !hasStop
  );
  const canRun = $derived(datasetIds.length > 0 && !stopMissing);

  const STEPS = $derived([
    { id: 'data', icon: 'database', label: $t('backtest.step.data'), summary: dataSummary(datasets, datasetIds, $t), ok: datasetIds.length > 0 },
    { id: 'strategy', icon: 'zap', label: $t('backtest.step.strategy'), summary: strategySummary(settings, $t), ok: true },
    { id: 'filters', icon: 'filter', label: $t('backtest.step.filters'), summary: filtersSummary(settings, $t), ok: true },
    { id: 'sizing', icon: 'wallet', label: $t('backtest.step.sizing'), summary: sizingSummary(settings, $t), ok: !stopMissing },
    { id: 'advanced', icon: 'settings', label: $t('backtest.step.advanced'), summary: advancedSummary(settings, $t), ok: true }
  ]);
  const stepIndex = $derived(STEPS.findIndex((s) => s.id === step));

  function goStep(delta) {
    const next = STEPS[stepIndex + delta];
    if (next) step = next.id;
  }
  function onStepKey(e, i) {
    const last = STEPS.length - 1;
    let n = null;
    if (e.key === 'ArrowRight') n = i === last ? 0 : i + 1;
    else if (e.key === 'ArrowLeft') n = i === 0 ? last : i - 1;
    else if (e.key === 'Home') n = 0;
    else if (e.key === 'End') n = last;
    if (n === null) return;
    e.preventDefault();
    step = STEPS[n].id;
  }

  // ── Loading ──
  async function loadDatasets() {
    datasets = await backtestApi.datasets();
  }
  async function loadRuns() {
    runs = await backtestApi.runs();
  }
  async function loadStrategies() {
    strategies = await backtestApi.strategies();
  }
  async function loadIndicators() {
    indicators = await indicatorLib.list();
  }
  // Paper sessions and the events nothing could be pushed to a channel for. The second list
  // is why the dock badge is right after a spell with the engine down: an undelivered fill is
  // read here, not lost.
  async function loadPaper() {
    const r = await paperApi.list();
    paperSessions = r.sessions ?? [];
    paperUnseen = await paperApi.events({ unseen: true });
  }
  $effect(() => {
    loadDatasets().catch((e) => (error = e.message));
    loadRuns().catch(() => {});
    loadStrategies().catch(() => {});
    loadIndicators().catch(() => {});
    loadOptJob().catch(() => {});
    loadPaper().catch(() => {});
    // The chart's display config is global (one per install), so a result chart looks like the
    // chart the user set up in the visualization module.
    histvizApi
      .chartSettings()
      .then((raw) => (chartSettings = normalizeChartSettings(raw)))
      .catch(() => {});
  });

  // ── Paper trading ──
  // The button freezes what is on screen: the settings exactly as they were run (indicators
  // embedded, so the session does not follow a later edit of the library) and the datasets
  // they were run over.
  function startPaper() {
    const payload = embedIndicators(normalizeSettings(settings), indicators);
    paperDraft = defaultSession(
      strategyName || $t('backtest.header.untitled'),
      [...datasetIds],
      payload
    );
    paperOpen = true;
  }
  function editPaper(session) {
    paperDraft = { ...session };
    paperOpen = true;
  }

  // ── Editing the strategy a session froze ──
  // The session's own form saves the run settings; the strategy is edited in the editor
  // that built it. Loading it here records where it came from, so the next strategy save
  // can offer to carry the change back into the session.
  let paperOrigin = $state(null); // { id, name, session }

  async function editPaperStrategy(session) {
    if (!(await confirmDiscard())) return;
    paperOpen = false;
    settings = migrateSettings(session.settings);
    datasetIds = [...(session.dataset_ids ?? [])];
    strategyId = session.strategy_id ?? null;
    if (strategyId) {
      const full = await backtestApi.getStrategy(strategyId).catch(() => null);
      strategyName = full?.name ?? session.name;
      strategyTags = [...(full?.tags ?? [])];
    } else {
      strategyName = '';
      strategyTags = [];
    }
    paperOrigin = { session };
    result = null;
    showResult = false;
    activeRunId = null;
    step = 'strategy';
    markClean();
  }

  // What to do with the session once its strategy has been rewritten: follow it, or leave
  // it alone and start a copy under the new name. Asked, never guessed: an active session
  // that silently changes strategy is a book describing a simulation that no longer ran.
  let applyOpen = $state(false);
  let applyBusy = $state(false);
  let applyName = $state('');

  function askApplyToSession() {
    if (!paperOrigin) return;
    applyName = strategyName.trim() || paperOrigin.session.name;
    applyOpen = true;
  }

  async function applyToSession(mode) {
    const origin = paperOrigin?.session;
    if (!origin) return;
    applyBusy = true;
    try {
      const payload = embedIndicators(normalizeSettings(settings), indicators);
      const next = {
        ...origin,
        name: applyName.trim() || origin.name,
        strategy_id: strategyId,
        settings: payload,
        dataset_ids: [...datasetIds]
      };
      if (mode === 'update') await paperApi.update(origin.id, next);
      else await paperApi.create({ ...next, id: undefined });
      applyOpen = false;
      paperOrigin = null;
      await loadPaper();
    } catch (e) {
      error = e.message;
    } finally {
      applyBusy = false;
    }
  }
  // One dot for the whole tab: red as soon as a session failed, green while one is running,
  // nothing when they are all paused. A count of what is running would say less than the dot.
  const paperDot = $derived(
    paperSessions.some((s) => s.status === 'error')
      ? 'err'
      : paperSessions.some((s) => s.status === 'active')
        ? 'ok'
        : ''
  );
  const paperTickers = $derived(
    (paperDraft?.dataset_ids ?? [])
      .map((id) => datasets.find((d) => d.id === id)?.ticker ?? '')
      .filter(Boolean)
  );

  // Alignment preview for any selection (one dataset reads it as its period). Sole dependency
  // is `datasetKey` (a primitive join of the ids); settings + indicators are read untracked so
  // strategy edits don't refetch alignment (no banner jump).
  const datasetKey = $derived(datasetIds.join(','));
  $effect(() => {
    datasetKey; // sole dependency
    const ids = untrack(() => datasetIds.slice());
    if (!ids.length) {
      alignment = null;
      return;
    }
    const snap = untrack(() => embedIndicators(normalizeSettings(settings), indicators));
    aligning = true;
    backtestApi
      .align(ids, snap)
      .then((a) => (alignment = a))
      .catch(() => (alignment = null))
      .finally(() => (aligning = false));
  });

  // ── Running ──
  // The Run button *is* the progress bar: a fill crossing it while the engine works. A run is
  // one blocking request with nothing to report from inside, so the fill is a curve over the
  // elapsed time, deliberately slow: it approaches the end without reaching it, and the result
  // lands on a button that is not full yet. Late reads as honest, early reads as broken.
  const SPEED_KEY = 'otw.backtest.speed.v1';
  /** Cold-start cost per bar, per strategy family (ms). Replaced by what this install measures
   *  on its own hardware from the first run on. */
  const DEFAULT_MS_PER_BAR = { signals: 0.06, grid: 0.5 };
  let runProgress = $state(0);
  let runTimer = 0;
  let runStarted = 0;

  const speedStore = () => {
    try {
      const raw = JSON.parse(localStorage.getItem(SPEED_KEY) || '{}');
      return raw && typeof raw === 'object' ? raw : {};
    } catch {
      return {};
    }
  };
  const median = (xs) => {
    const s = [...xs].sort((a, b) => a - b);
    return s.length ? s[Math.floor(s.length / 2)] : null;
  };
  /** Bars the engine will walk: every selected dataset, as the catalog counts them. */
  const runBars = () =>
    datasetIds.reduce((n, id) => n + (datasets.find((d) => d.id === id)?.bar_count ?? 0), 0);

  /** How long this run is likely to take, from what past runs of the same family measured. */
  function estimateMs() {
    const kind = settings.kind === 'grid' ? 'grid' : 'signals';
    const perBar = median(speedStore()[kind] ?? []) ?? DEFAULT_MS_PER_BAR[kind];
    const conditions =
      [settings.long, settings.short].filter(Boolean).reduce(
        (n, s) => n + (s.entry?.conditions?.length ?? 0) + (s.exit?.conditions?.length ?? 0),
        0
      ) || 1;
    return Math.max(400, runBars() * perBar * (1 + 0.15 * (conditions - 1)));
  }

  /** Record what this run really cost, so the next one is paced better. */
  function recordSpeed(ms) {
    const bars = runBars();
    if (bars < 500) return; // too small to measure anything but the round trip
    const kind = settings.kind === 'grid' ? 'grid' : 'signals';
    const store = speedStore();
    store[kind] = [...(store[kind] ?? []), ms / bars].slice(-20);
    try {
      localStorage.setItem(SPEED_KEY, JSON.stringify(store));
    } catch {
      /* the next run simply falls back to the default pace */
    }
  }

  function startProgress() {
    // 1 - exp(-t/tau) never reaches 1, and tau is stretched past the estimate on purpose: a
    // run that beats its estimate stops early on a half-filled button, which is the good way
    // round. 0.97 is the ceiling, so the button is never full before the answer.
    const tau = estimateMs() * 0.9;
    runStarted = performance.now();
    runProgress = 0;
    clearInterval(runTimer);
    runTimer = setInterval(() => {
      runProgress = Math.min(0.97, 1 - Math.exp(-(performance.now() - runStarted) / tau));
    }, 120);
  }
  function stopProgress(done) {
    clearInterval(runTimer);
    runTimer = 0;
    if (!done) {
      runProgress = 0;
      return;
    }
    runProgress = 1;
    setTimeout(() => (runProgress = 0), 220);
  }
  onDestroy(() => clearInterval(runTimer));

  /** @param replay a refresh replaying the run that was on screen: nothing new to record, and
   *  the view it had (tab, charted asset, chart series) is restored instead of reset. */
  async function run(replay = null) {
    if (!canRun) return;
    error = '';
    running = true;
    startProgress();
    try {
      const payload = embedIndicators(normalizeSettings(settings), indicators);
      const assetId = replay?.chartAssetId && datasetIds.includes(replay.chartAssetId)
        ? replay.chartAssetId
        : datasetIds[0];
      const [res, b] = await Promise.all([
        backtestApi.run(datasetIds, payload, { record: !replay }),
        backtestApi.bars(assetId)
      ]);
      result = res;
      bars = b;
      chartAssetId = assetId;
      // A replay is the same run as the recorded one: keep pointing at that row, so the report
      // and "Save results" still have something to pin to.
      if (replay) result.run_id ??= replay.activeRunId ?? null;
      else activeRunId = res.run_id ?? null;
      showResult = true;
      tab = replay?.tab ?? 'summary';
      pickedTrade = null;
      assetView = replay?.assetView ?? '';
      // Seed the chart with what the strategy reads. Re-seeded on every run, so a parameter
      // change is on screen next time: this is the run's chart, not a saved layout.
      chartInstances = replay?.chartInstances?.length
        ? replay.chartInstances
        : strategyInstances(settings, indicators);
      nextInstanceId = chartInstances.reduce((m, i) => Math.max(m, i.id ?? 0), 0) + 1;
      recordSpeed(performance.now() - runStarted);
      stopProgress(true);
      if (!replay) loadRuns().catch(() => {});
    } catch (e) {
      error = e.message;
      stopProgress(false);
    } finally {
      running = false;
    }
  }

  const chartAssets = $derived(
    datasetIds
      .map((id) => datasets.find((d) => d.id === id))
      .filter(Boolean)
      .map((d) => ({ id: d.id, ticker: d.ticker }))
  );
  async function switchChartAsset(id) {
    const nid = chartAssets.find((a) => String(a.id) === String(id))?.id ?? id;
    chartAssetId = nid;
    try {
      bars = await backtestApi.bars(nid);
    } catch (e) {
      error = e.message;
    }
  }

  // ── From the trade list to the chart ──
  // Picking a trade frames it on the chart: the right asset first (a portfolio run charts one
  // at a time), then a window sized from the trade itself, so a five-bar scalp and a
  // three-week swing both open readable.
  let pickedTrade = $state(null);
  let chartRef = $state(null);
  /** Bars shown around a trade, at least this many. */
  const TRADE_MIN_BARS = 60;

  async function focusTrade(tr) {
    if (!tr) return;
    pickedTrade = tr.n ?? null;
    // A trade of another asset needs that asset's candles before it can be framed.
    if (tr.ticker && chartAssets.length > 1 && tr.ticker !== chartTicker) {
      const asset = chartAssets.find((a) => a.ticker === tr.ticker);
      if (asset) await switchChartAsset(asset.id);
    }
    // Bring the chart back if it was dragged shut or hidden.
    chartHidden = false;
    if (chartH < 240) {
      chartH = 380;
      persistChartH();
    }
    // The chart resets its window whenever its bars change (mount, asset switch), and that
    // happens on the flush this awaits: frame the trade after it, or it would be undone.
    await tick();
    await new Promise((r) => requestAnimationFrame(r));
    chartRef?.focusRange?.(Date.parse(tr.entry_ts), Date.parse(tr.exit_ts), { minBars: TRADE_MIN_BARS });
  }

  // ── Chart indicators ──
  // Seeded from the strategy, then the user's to manage: hiding, editing or adding a series is
  // a reading gesture and never writes back to the strategy.
  function saveInstance(draft) {
    const body = {
      type: draft.type,
      params: draft.params,
      style: draft.style,
      ...(draft.type === 'custom' ? { custom: draft.custom, pane: draft.pane } : {})
    };
    if (editingInstance) {
      chartInstances = chartInstances.map((i) => (i.id === editingInstance.id ? { ...i, ...body } : i));
    } else {
      chartInstances = [...chartInstances, { id: nextInstanceId++, ...body, visible: true }];
    }
    editingInstance = null;
  }
  const toggleInstance = (id) =>
    (chartInstances = chartInstances.map((i) => (i.id === id ? { ...i, visible: !i.visible } : i)));
  const removeInstance = (id) => (chartInstances = chartInstances.filter((i) => i.id !== id));

  // ── Chart / results split ──
  // A drag between the two panes, kept in this browser: how much chart a user wants is a
  // habit, not a per-run choice.
  function persistChartH() {
    try {
      localStorage.setItem(CHART_H_KEY, String(Math.round(chartH)));
    } catch {
      /* quota: the split still holds for this session */
    }
  }
  let grip = $state(null);
  let gripDrag = $state(null);
  /** A banner drag ends on a click: past a few pixels it was a resize, not a restore. */
  let gripMoved = false;
  /** Under this the pane is not a chart any more, so it collapses instead of leaving a sliver
   *  (and ECharts never gets a zero-height box to measure). */
  const CHART_MIN = 48;
  /** Same idea below the bar: a few pixels of statistics are no statistics. */
  const DETAILS_MIN = 40;
  /** The bar is a hairline while both sides are open, and a labelled banner once one is shut,
   *  since that is then the only way back. */
  const GRIP_H = 9;
  const GRIP_BANNER_H = 30;
  /** What a restored split gives the chart. */
  const SPLIT_RATIO = 0.6;

  // The split's own height, watched: which side is shut is a fact about the layout, not about
  // the number the user last dragged to.
  let splitH = $state(0);
  $effect(() => {
    const el = splitEl;
    if (!el) return;
    const ro = new ResizeObserver(() => (splitH = el.clientHeight));
    ro.observe(el);
    splitH = el.clientHeight;
    return () => ro.disconnect();
  });

  const chartOff = $derived(!chartHidden && chartH < CHART_MIN);
  const detailsOff = $derived(
    !chartHidden && splitH > 0 && chartH > splitH - GRIP_BANNER_H - DETAILS_MIN
  );
  const gripH = $derived(chartOff || detailsOff ? GRIP_BANNER_H : GRIP_H);

  /** The drag runs edge to edge: all the way up hides the chart, all the way down hides the
   *  statistics. Only the grab bar always keeps its own row, so the split can come back. */
  const clampChartH = (h) => Math.max(0, Math.min(h, (splitEl?.clientHeight ?? 800) - gripH));

  /** Back to a working split from either banner. */
  function restoreSplit() {
    chartHidden = false;
    chartH = Math.round(((splitH || splitEl?.clientHeight || 800) - GRIP_H) * SPLIT_RATIO);
    persistChartH();
  }

  function onGripDown(e) {
    if (e.button !== 0) return;
    gripDrag = { y0: e.clientY, h0: chartH };
    gripMoved = false;
    grip?.setPointerCapture?.(e.pointerId);
    e.preventDefault();
  }
  function onGripMove(e) {
    if (!gripDrag) return;
    if (Math.abs(e.clientY - gripDrag.y0) > 3) gripMoved = true;
    chartH = clampChartH(gripDrag.h0 + (e.clientY - gripDrag.y0));
  }
  function onGripUp(e) {
    if (!gripDrag) return;
    gripDrag = null;
    grip?.releasePointerCapture?.(e.pointerId);
    persistChartH();
  }
  function onGripKey(e) {
    const step = e.shiftKey ? 60 : 20;
    if (e.key === 'ArrowUp') chartH = clampChartH(chartH - step);
    else if (e.key === 'ArrowDown') chartH = clampChartH(chartH + step);
    else return;
    e.preventDefault();
    persistChartH();
  }

  // ── Dock size ──
  // The dock is the page's other half: slid from its top edge to whatever share of the screen
  // the user wants, clicked shut when only the wizard matters. Kept in this browser, like the
  // chart split: it is a habit, not a per-run choice.
  const DOCK_H_KEY = 'otw.backtest.dockH';
  const DOCK_OPEN_KEY = 'otw.backtest.dockOpen';
  /** Under this the list is not a list any more. */
  const DOCK_MIN = 90;
  /** What the wizard always keeps for itself, however far the dock is pulled up. */
  const WORK_MIN = 220;
  const readDockH = () => {
    const v = Number(localStorage.getItem(DOCK_H_KEY));
    return Number.isFinite(v) && v >= DOCK_MIN ? v : 0; // 0 = the 35% default
  };
  let dockH = $state(readDockH());
  let dockOpen = $state(localStorage.getItem(DOCK_OPEN_KEY) !== '0');
  let dockEl = $state(null);
  let dockGrip = $state(null);
  let dockDrag = $state(null);

  const clampDockH = (h) =>
    Math.max(DOCK_MIN, Math.min(h, Math.max(DOCK_MIN, (pageEl?.clientHeight ?? 800) - WORK_MIN)));

  function persistDock() {
    try {
      localStorage.setItem(DOCK_H_KEY, String(Math.round(dockH)));
      localStorage.setItem(DOCK_OPEN_KEY, dockOpen ? '1' : '0');
    } catch {
      /* quota: the dock still holds for this session */
    }
  }

  function toggleDock() {
    dockOpen = !dockOpen;
    if (dockOpen && !dockH) dockH = clampDockH(Math.round((pageEl?.clientHeight ?? 800) * 0.35));
    persistDock();
  }

  function onDockDown(e) {
    if (e.button !== 0) return;
    dockDrag = {
      y0: e.clientY,
      h0: dockOpen ? dockH || dockEl?.clientHeight || 300 : DOCK_MIN,
      moved: false
    };
    dockGrip?.setPointerCapture?.(e.pointerId);
    e.preventDefault();
  }
  function onDockMove(e) {
    if (!dockDrag) return;
    const dy = dockDrag.y0 - e.clientY; // up = taller
    if (!dockDrag.moved && Math.abs(dy) < 3) return;
    dockDrag.moved = true;
    dockOpen = true; // sliding a shut dock is how it comes back
    dockH = clampDockH(dockDrag.h0 + dy);
  }
  function onDockUp(e) {
    if (!dockDrag) return;
    const { moved } = dockDrag;
    dockDrag = null;
    dockGrip?.releasePointerCapture?.(e.pointerId);
    // A press that never slid is a click: that shuts or reopens the dock.
    if (moved) persistDock();
    else toggleDock();
  }
  function onDockKey(e) {
    const step = e.shiftKey ? 60 : 20;
    if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
      dockOpen = true;
      dockH = clampDockH((dockH || dockEl?.clientHeight || 300) + (e.key === 'ArrowUp' ? step : -step));
    } else if (e.key === 'Enter' || e.key === ' ') {
      toggleDock();
      e.preventDefault();
      return;
    } else return;
    e.preventDefault();
    persistDock();
  }

  // ── Strategies ──
  // Loading anything over an edited draft would drop it — ask first. The answer comes back
  // from a modal, so the callers await it.
  let discardOpen = $state(false);
  let discardAnswer = null; // resolver of the pending confirmDiscard()
  const confirmDiscard = () => {
    if (!dirty) return Promise.resolve(true);
    discardOpen = true;
    return new Promise((resolve) => (discardAnswer = resolve));
  };
  function answerDiscard(ok) {
    const resolve = discardAnswer;
    discardAnswer = null;
    resolve?.(ok);
  }

  async function newStrategy() {
    if (!(await confirmDiscard())) return;
    settings = defaultSettings();
    datasetIds = [];
    strategyId = null;
    strategyName = '';
    strategyTags = [];
    result = null;
    showResult = false;
    activeRunId = null;
    step = 'data';
    markClean();
  }

  async function openStrategy(s) {
    if (!(await confirmDiscard())) return;
    const full = await backtestApi.getStrategy(s.id).catch(() => s);
    settings = migrateSettings(full.settings);
    strategyId = full.id;
    strategyName = full.name;
    strategyTags = [...(full.tags ?? [])];
    result = null;
    showResult = false;
    activeRunId = null;
    markClean();
  }

  // Save = create or update. An unnamed draft needs the modal first (name + tags), and so
  // does a strategy opened from a paper session: overwriting the row that session points
  // at is a decision, not the default a Save button takes on its own.
  function saveStrategy() {
    strategyError = '';
    if (!strategyName.trim() || (paperOrigin && strategyId)) {
      strategyModalOpen = true;
      return;
    }
    commitStrategy();
  }

  /** Write the draft to the library. `edit` carries the modal's name/tags when it saved. */
  async function commitStrategy(edit) {
    // "Save as new" from the modal: forget the row we came from, so the write creates one.
    if (edit && edit.overwrite === false) strategyId = null;
    if (edit) {
      strategyName = edit.name;
      strategyTags = edit.tags;
    }
    const name = strategyName.trim();
    if (!name) {
      strategyError = $t('backtest.save.nameRequired');
      return;
    }
    // Persist the raw editor state (reverse_side toggle + both sides) so a strategy round-trips
    // losslessly. normalizeSettings is a run-time transform (derives/drops sides) — applying it
    // here would silently strip settings the user configured. Indicators are still embedded so
    // the saved strategy is self-contained.
    const payload = embedIndicators(migrateSettings(settings), indicators);
    try {
      if (strategyId) await backtestApi.updateStrategy(strategyId, name, '', strategyTags, payload);
      else {
        const { id } = await backtestApi.createStrategy(name, '', strategyTags, payload);
        strategyId = id;
      }
      markClean();
      strategyModalOpen = false;
      strategyError = '';
      await loadStrategies();
      // Saved from a paper session: ask what happens to the session before moving on.
      if (paperOrigin) askApplyToSession();
    } catch (e) {
      strategyError = e.message;
      if (!strategyModalOpen) error = e.message;
    }
  }

  async function duplicateStrategy(s) {
    const full = await backtestApi.getStrategy(s.id).catch(() => s);
    try {
      await backtestApi.createStrategy(
        $t('backtest.dock.copyName', { name: full.name }),
        full.description ?? '',
        full.tags ?? [],
        full.settings
      );
      await loadStrategies();
    } catch (e) {
      error = e.message;
    }
  }

  async function deleteStrategy(s) {
    if (!confirm($t('backtest.dock.deleteStrategyConfirm', { name: s.name }))) return;
    await backtestApi.deleteStrategy(s.id).catch(() => {});
    if (strategyId === s.id) strategyId = null; // the draft lives on, as an unsaved one
    await loadStrategies();
  }

  // ── History ──
  async function loadRun(r) {
    if (!(await confirmDiscard())) return;
    settings = migrateSettings(r.settings);
    datasetIds = r.dataset_ids?.length ? [...r.dataset_ids] : r.dataset_id ? [r.dataset_id] : [];
    strategyId = r.strategy_id ?? null; // provenance: saving writes back to that strategy
    result = null;
    showResult = false;
    activeRunId = r.id;
    error = datasetIds.length ? '' : $t('backtest.page.datasetDeletedErr');
    markClean();
  }

  async function removeRun(id) {
    await backtestApi.remove(id).catch(() => {});
    if (activeRunId === id) activeRunId = null;
    await loadRuns();
  }

  // Clearing the run history touches `backtest_runs` and nothing else: a paper session holds
  // its own frozen copy of the settings, so it is unaffected.
  let clearHistoryOpen = $state(false);

  async function ackPaper() {
    try {
      await paperApi.markSeen(null);
      await loadPaper();
    } catch (e) {
      error = e.message;
    }
  }

  async function clearHistory() {
    try {
      await backtestApi.clearHistory();
      await loadRuns();
    } catch (e) {
      error = e.message;
    }
  }

  const openReport = (id) => goto(`/backtest/report/${id}`);

  // Restore a run and show its result (used when returning from the report via ?run=<id>).
  async function restoreRun(id) {
    const list = runs.length ? runs : await backtestApi.runs().catch(() => []);
    const r = list.find((x) => x.id === id);
    if (!r) return;
    loadRun(r);
    if (datasetIds.length) await run();
  }
  // Honor ?run=<id> once (from the report Back link), then strip it from the URL.
  let restored = $state(false);
  $effect(() => {
    const rid = $page.url.searchParams.get('run');
    if (rid && !restored) {
      restored = true;
      restoreRun(rid).finally(() => goto('/backtest', { replaceState: true, keepFocus: true, noScroll: true }));
    }
  });

  // ?dataset=<id> — the chart hands the instrument over so the strategy opens on it already
  // selected. Waits for the catalog, since the id has to exist to be selectable.
  let deepLinked = $state(false);
  $effect(() => {
    const did = $page.url.searchParams.get('dataset');
    if (!did || deepLinked || !datasets.length) return;
    deepLinked = true;
    if (datasets.some((d) => d.id === did)) datasetIds = [did];
    goto('/backtest', { replaceState: true, keepFocus: true, noScroll: true });
  });

  // ── Surviving a refresh ──
  // The page lives in memory, so a reload used to drop the draft, the run and the view. The
  // session is written to sessionStorage (per tab) on the way out of the *document* and read
  // back on load; a result that was open is replayed from its own settings, which is
  // deterministic and, being the same run, records no second history entry.
  //
  // Leaving the module clears it: this is a reload surviving its own reload, not a workspace.
  const SESSION_KEY = 'otw.backtest.session.v1';
  let unloading = false;

  function saveSession() {
    try {
      sessionStorage.setItem(
        SESSION_KEY,
        JSON.stringify({
          settings: $state.snapshot(settings),
          datasetIds: $state.snapshot(datasetIds),
          strategyId,
          strategyName,
          strategyTags: $state.snapshot(strategyTags),
          baseline,
          step,
          dockTab,
          hasResult: showResult && !!result,
          // The result's headline, kept so a reload paints the finished screen on its first
          // frame instead of the builder: a couple of kilobytes, no trade list, no curve.
          summary: showResult && result
            ? {
                ticker: result.ticker,
                timeframe: result.timeframe,
                bars: result.bars,
                warmup_bars: result.warmup_bars,
                trading_start_ts: result.trading_start_ts ?? null,
                stats: result.stats,
                per_asset: result.per_asset ?? [],
                trades: result.trades.length,
                grid: result.grid ?? null
              }
            : null,
          tab,
          assetView,
          activeRunId,
          chartAssetId,
          chartHidden,
          chartInstances: $state.snapshot(chartInstances)
        })
      );
    } catch {
      /* quota or private mode: the next load simply starts fresh */
    }
  }
  function onUnload() {
    unloading = true;
    saveSession();
  }
  onDestroy(() => {
    if (unloading) return; // a reload: the session is for the load that follows
    try {
      sessionStorage.removeItem(SESSION_KEY);
    } catch {
      /* nothing to clean */
    }
  });

  let sessionRead = false;
  $effect(() => {
    if (sessionRead) return;
    sessionRead = true;
    // A deep link is an explicit instruction: it wins over whatever this tab was doing.
    const url = untrack(() => $page.url);
    if (url.searchParams.get('run') || url.searchParams.get('dataset')) return;
    let s = null;
    try {
      s = JSON.parse(sessionStorage.getItem(SESSION_KEY) || 'null');
    } catch {
      s = null;
    }
    if (!s || typeof s !== 'object') return;
    settings = migrateSettings(s.settings ?? defaultSettings());
    datasetIds = Array.isArray(s.datasetIds) ? s.datasetIds : [];
    strategyId = s.strategyId ?? null;
    strategyName = s.strategyName ?? '';
    strategyTags = Array.isArray(s.strategyTags) ? s.strategyTags : [];
    baseline = s.baseline ?? draftKey();
    step = s.step ?? 'data';
    dockTab = s.dockTab ?? 'history';
    chartHidden = !!s.chartHidden;
    activeRunId = s.activeRunId ?? null;
    if (!s.hasResult || !datasetIds.length) return;
    // Paint the finished screen at once, from the headline that was kept: the numbers are the
    // run's own, the heavy parts (candles, trades, curves) wait behind a skeleton while the
    // replay recomputes them. The builder is never shown, so a reload reads as a restore and
    // not as a fresh run.
    restoring = s.summary ?? null;
    tab = s.tab ?? 'summary';
    assetView = s.assetView ?? '';
    chartAssetId = s.chartAssetId ?? null;
    showResult = true;
    // The custom-indicator library has to be in before the replay: the run embeds the
    // definitions it reads, and an empty library would silently run a different strategy.
    loadIndicators()
      .catch(() => {})
      .then(() => run(s))
      .finally(() => (restoring = null));
  });

  // ── Reports ──
  const reportName = $derived(
    result
      ? (result.per_asset?.length > 1 ? result.per_asset.map((a) => a.ticker).join('-') : result.ticker) +
          ` ${result.timeframe}`
      : 'backtest'
  );

  function downloadReport() {
    if (!result) return;
    const md = buildReportMd({
      name: reportName,
      settings: embedIndicators(normalizeSettings(settings), indicators),
      result
    });
    downloadText(reportFilename(strategyName), md);
  }

  // Naming a run pins it: it stops being an auto history entry and survives the 300 cap.
  async function saveRun() {
    const name = saveRunName.trim();
    if (!name || !result) return;
    try {
      await backtestApi.save(name, result.run_id, {
        datasetIds,
        settings: normalizeSettings(settings),
        stats: result.stats,
        strategyId
      });
      saveRunOpen = false;
      saveRunName = '';
      dockTab = 'history';
      await loadRuns();
    } catch (e) {
      error = e.message;
    }
  }
</script>

<RequireModule module="backtest">
<div class="page" bind:this={pageEl}>
  <header class="top">
    <h1>{$t('backtest.page.title')}</h1>
    <div class="ident">
      <button class="name" onclick={() => { strategyError = ''; strategyModalOpen = true; }}
        title={$t('backtest.header.rename')}>
        <Icon name="zap" size={12} />
        <span class:untitled={!strategyName}>{strategyName || $t('backtest.header.untitled')}</span>
        {#if dirty}<span class="dot" title={$t('backtest.header.unsaved')}></span>{/if}
      </button>
      {#each strategyTags as tag (tag)}<span class="tag">{tag}</span>{/each}
    </div>
    <div class="acts">
      {#if optJob}
        <!-- A sweep started here is still running: it lives on the server, so the way back to it
             is a link and not a restore. -->
        <button class="opt-live" onclick={() => goto(`/backtest/optimize/${optJob.id}`)}>
          <span class="pulse"></span>
          {$t('backtest.opt.resume', {
            done: fmtNum(optJob.progress?.done ?? 0, 0),
            total: fmtNum(optJob.total ?? 0, 0)
          })}
        </button>
      {/if}
      <button onclick={newStrategy}><Icon name="plus" size={13} /> {$t('backtest.header.new')}</button>
      <button class="primary" onclick={saveStrategy}><Icon name="save" size={13} /> {$t('backtest.header.save')}</button>
    </div>
  </header>

  {#if error}<p class="err" title={$t('backtest.page.clickToCopy')} use:copyLog={error}>{error}</p>{/if}

  <!-- ── Work area: the wizard, or the result of a run ── -->
  <section class="work">
    {#if showResult && shown}
      <div class="result-head">
        <button class="back" onclick={() => (showResult = false)}><Icon name="chevron-left" size={13} /> {$t('backtest.result.edit')}</button>
        {#if chartAssets.length > 1}
          <!-- A portfolio run can carry eight assets: a picker, not a strip of buttons. -->
          <div class="pick asset">
            <Dropdown
              value={chartAssetId}
              options={chartAssets.map((a) => ({ value: a.id, label: a.ticker }))}
              ariaLabel={$t('backtest.chart.asset')}
              title={$t('backtest.chart.asset')}
              onpick={switchChartAsset}
            />
          </div>
        {:else}
          <span class="title">{shown.ticker}</span>
        {/if}
        <span class="chip">{shown.timeframe}</span>
        <span class="sub">{$t('backtest.page.barsCount', { count: shown.bars?.toLocaleString?.() ?? '' })}</span>
        {#if shown.warmup_bars > 0}
          <span class="sub" title={shown.trading_start_ts ?? ''}>{$t('backtest.page.warmup', { n: shown.warmup_bars })}</span>
        {/if}
        {#if restoring}<span class="sub restoring">{$t('backtest.page.restoring')}</span>{/if}
        <div class="head-acts">
          <button onclick={() => (chartHidden = !chartHidden)}>
            <Icon name={chartHidden ? 'chevron-down' : 'chevron-up'} size={13} />
            {chartHidden ? $t('backtest.page.showChart') : $t('backtest.page.hideChart')}
          </button>
          {#if !chartHidden}
            <button onclick={() => { editingInstance = null; indicatorOpen = true; }}>
              <Icon name="trending-up" size={13} /> {$t('backtest.chart.indicators')}
              {#if chartInstances.length}<span class="count">{chartInstances.length}</span>{/if}
            </button>
          {/if}
          <!-- Optimizer: vary the parameters of the strategy that just ran, over the same data. -->
          <button disabled={!result} onclick={() => (optimizeOpen = true)}>
            <Icon name="target" size={13} /> {$t('backtest.opt.open')}
          </button>
          <!-- Run it forward: same strategy, same instruments, on a schedule. -->
          <button disabled={!result} onclick={startPaper}>
            <Icon name="activity" size={13} /> {$t('backtest.paper.start')}
          </button>
          <!-- The two ways out of a run are one menu: same subject, one slot in the header. -->
          <div class="menu-wrap" use:clickOutside={() => (reportMenu = false)}>
            <button
              disabled={!result}
              aria-haspopup="menu"
              aria-expanded={reportMenu}
              onclick={() => (reportMenu = !reportMenu)}
            >
              <Icon name="file-text" size={13} /> {$t('backtest.report.menu')}
              <Icon name="chevron-down" size={11} />
            </button>
            {#if reportMenu}
              <div class="menu" role="menu">
                <button role="menuitem" onclick={() => { reportMenu = false; downloadReport(); }}>
                  <Icon name="download" size={13} /> {$t('backtest.report.downloadMd')}
                </button>
                <button
                  role="menuitem"
                  onclick={() => { reportMenu = false; result?.run_id && openReport(result.run_id); }}
                >
                  <Icon name="file-text" size={13} /> {$t('backtest.report.printPdf')}
                </button>
              </div>
            {/if}
          </div>
          <button class="primary" disabled={!result} onclick={() => (saveRunOpen = true)}><Icon name="star" size={13} /> {$t('backtest.page.saveResults')}</button>
        </div>
      </div>

      <!-- Chart over results, split on a grab bar: the candles keep their own height while the
           statistics scroll under them. -->
      <div class="split" bind:this={splitEl}>
        {#if !chartHidden && !bars?.ts?.length && restoring}
          <!-- The candles are on their way back: hold their box rather than let the page jump. -->
          <div class="chart-pane" style:height="min({chartH}px, calc(100% - {gripH}px))" aria-busy="true">
            <Skeleton height="100%" />
          </div>
        {/if}
        {#if !chartHidden && bars?.ts?.length}
          {#if chartFull || chartH >= CHART_MIN}
          <div
            class="chart-pane"
            class:fullscreen={chartFull}
            style:height={chartFull ? '100%' : `min(${chartH}px, calc(100% - ${gripH}px))`}
          >
            {#if chartFull}
              <button class="fs-close" title={$t('backtest.chart.exitFullscreen')} onclick={() => (chartFull = false)}>
                <Icon name="x" size={13} />
              </button>
            {/if}
            <Chart
              bind:this={chartRef}
              {bars}
              instances={chartInstances}
              settings={chartSettings}
              fullscreen={chartFull}
              {marks}
              markSizes={false}
              title={{ ticker: chartTicker ?? shown.ticker, timeframe: shown.timeframe, provider: bars?.provider ?? '' }}
              ontoggle={toggleInstance}
              onedit={(id) => { editingInstance = chartInstances.find((i) => i.id === id) ?? null; indicatorOpen = true; }}
              onremove={removeInstance}
              onfullscreen={() => (chartFull = !chartFull)}
            />
          </div>
          {/if}
          <!-- The grab bar. Dragged shut, it becomes the banner that says what is missing and
               brings the split back, since it is then the only way back. -->
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <div
            class="grip"
            class:dragging={!!gripDrag}
            class:banner={chartOff || detailsOff}
            class:up={chartOff}
            bind:this={grip}
            role="separator"
            tabindex="0"
            aria-orientation="horizontal"
            aria-label={$t('backtest.chart.resize')}
            title={$t('backtest.chart.resize')}
            onpointerdown={onGripDown}
            onpointermove={onGripMove}
            onpointerup={onGripUp}
            onpointercancel={onGripUp}
            onkeydown={onGripKey}
          >
            {#if chartOff || detailsOff}
              <button class="restore" onclick={() => !gripMoved && restoreSplit()}>
                <Icon name={chartOff ? 'chevron-down' : 'chevron-up'} size={11} />
                <span>{chartOff ? $t('backtest.page.showChart') : $t('backtest.page.showDetails')}</span>
              </button>
            {/if}
          </div>
        {/if}

        <!-- The padding lives on the inner box: dragged shut, an empty padded strip would still
             hold the bottom of the screen. -->
        <div class="result-body">
          <div class="result-inner">
          <StatsGrid stats={shown.stats} dca={shown.dca ?? false} />

          {#if result?.skipped_min_size || result?.skipped_margin || result?.halted_bars || result?.filtered_bars}
            <div class="badges">
              {#if result.skipped_min_size}<span class="badge">{$t('backtest.page.skippedMin', { n: result.skipped_min_size })}</span>{/if}
              {#if result.skipped_margin}<span class="badge">{$t('backtest.page.skippedMargin', { n: result.skipped_margin })}</span>{/if}
              {#if result.halted_bars}<span class="badge warn">{$t('backtest.page.halted', { n: result.halted_bars })}</span>{/if}
              {#if result.filtered_bars}<span class="badge">{$t('backtest.page.filtered', { n: result.filtered_bars })}</span>{/if}
            </div>
          {/if}

          <div class="tabs" role="tablist" aria-label={$t('backtest.page.resultDetail')}>
            <button role="tab" aria-selected={tab === 'summary'} onclick={() => (tab = 'summary')}>{$t('backtest.page.performanceSummary')}</button>
            <button role="tab" aria-selected={tab === 'stats'} onclick={() => (tab = 'stats')}>{$t('backtest.result.statistics')}</button>
            <button role="tab" aria-selected={tab === 'perf'} onclick={() => (tab = 'perf')}>{$t('backtest.result.performance')}</button>
            {#if shown.dca}
              <button role="tab" aria-selected={tab === 'plan'} onclick={() => (tab = 'plan')}>{$t('backtest.dca.planTab')}</button>
            {/if}
            {#if shown.per_asset?.length > 1}
              <button role="tab" aria-selected={tab === 'assets'} onclick={() => (tab = 'assets')}>{$t('backtest.asset.title')} <span class="count">{shown.per_asset.length}</span></button>
            {/if}
            <button role="tab" aria-selected={tab === 'trades'} onclick={() => (tab = 'trades')}>{$t('backtest.page.listOfTrades')} <span class="count">{tradeTabCount}</span></button>
          </div>

          {#if tab === 'summary'}
            <!-- The summary is the one tab a restore can serve whole: it is the run's headline,
                 which is exactly what was kept. -->
            {#if shown.grid}
              <div class="grid-stats">
                <span class="cap">{$t('backtest.grid.statsTitle')}</span>
                <span>{$t('backtest.grid.fills')}: <b>{shown.grid.fills}</b></span>
                <span>{$t('backtest.grid.roundTrips')}: <b>{shown.grid.round_trips}</b></span>
                <span>{$t('backtest.grid.endInventory')}: <b>{fmtNum(shown.grid.end_inventory, 4)}</b></span>
              </div>
            {/if}
            {#if shown.dca}<DcaPanel dca={shown.dca} compact />{/if}
            {#if result?.oos}<OosBlock oos={result.oos} />{/if}
            <PerfTable stats={shown.stats} dca={!!shown.dca} />
          {:else if !result}
            <!-- Everything else is derived from the trade list, which the replay is recomputing. -->
            <div class="sk-body" aria-busy="true"><Skeleton height="320px" /></div>
          {:else if tab === 'stats'}
            <!-- Closed-trade statistics: the same read as the chart's quick backtest. A plan
                 that only accumulates has no closed trade, so what it holds is the statistic. -->
            {#if dca}
              <DcaHoldings assets={dca.assets ?? []} perAsset={result.per_asset ?? []} asOf={lastBarTs} />
            {/if}
            {#if !dca || analysisTrades.length}
              <TradeStats s={closedStats} trades={analysisTrades} />
            {:else}
              <p class="empty-note">{$t('backtest.dca.noClosed')}</p>
            {/if}
          {:else if tab === 'perf'}
            {#if !dca || analysisTrades.length}
              <div class="curve-box"><TradeCurve trades={analysisTrades} /></div>
            {:else}
              <p class="empty-note">{$t('backtest.dca.noClosedCurve')}</p>
            {/if}
          {:else if tab === 'plan'}
            <DcaPanel dca={result.dca ?? shown.dca} perAsset={result.per_asset ?? []} asOf={lastBarTs} />
          {:else if tab === 'assets'}
            <!-- The table compares the assets; the picker reads one of them, with the same
                 statistics and performance views the whole run gets. -->
            <div class="asset-bar">
              <div class="pick asset">
                <Dropdown
                  bind:value={assetView}
                  options={[
                    { value: '', label: $t('backtest.asset.all') },
                    ...(result.per_asset ?? []).map((a) => ({ value: a.ticker, label: a.ticker }))
                  ]}
                  ariaLabel={$t('backtest.asset.ticker')}
                  title={$t('backtest.asset.ticker')}
                />
              </div>
              {#if dcaAssetRow}
                <!-- A savings plan is read by what it holds, not by round trips it never made. -->
                <span class="sub">{$t('backtest.dca.units')} <b>{fmtNum(dcaAssetRow.units, 4)}</b></span>
                <span class="sub">{$t('backtest.dca.value')} <b>{fmtNum(dcaAssetRow.value)}</b></span>
                <span class="sub">{$t('backtest.dca.unrealizedCol')}
                  <b class:pos={dcaAssetRow.unrealized_pnl > 0} class:neg={dcaAssetRow.unrealized_pnl < 0}
                    >{fmtNum(dcaAssetRow.unrealized_pnl)}</b></span>
                <span class="sub">{$t('backtest.asset.fees')} <b>{fmtNum(dcaAssetRow.fees)}</b></span>
              {:else if assetRow}
                <span class="sub">{$t('backtest.asset.trades')} <b>{fmtNum(assetRow.trades, 0)}</b></span>
                <span class="sub">{$t('backtest.asset.netPnl')}
                  <b class:pos={assetRow.net_pnl > 0} class:neg={assetRow.net_pnl < 0}>{fmtNum(assetRow.net_pnl)}</b></span>
                <span class="sub">{$t('backtest.asset.fees')} <b>{fmtNum(assetRow.total_fees)}</b></span>
                <span class="sub">{$t('backtest.asset.exposure')} <b>{fmtNum(assetRow.exposure_pct, 1)}%</b></span>
              {/if}
            </div>
            {#if dca}
              <DcaHoldings
                assets={dca.assets ?? []}
                perAsset={result.per_asset ?? []}
                ticker={assetView}
                asOf={lastBarTs} />
              {#if assetView}
                <DcaFills events={dca.events ?? []} total={dca.events_total} ticker={assetView} />
              {/if}
              {#if assetTrades.length}
                <TradeStats s={assetStats} trades={assetTrades} />
                <div class="curve-box"><TradeCurve trades={assetTrades} /></div>
              {/if}
            {:else if assetRow}
              <TradeStats s={assetStats} trades={assetTrades} />
              <div class="curve-box"><TradeCurve trades={assetTrades} /></div>
            {:else}
              <AssetBreakdown perAsset={result.per_asset ?? []} />
            {/if}
          {:else}
            <!-- A savings plan lists its fills: the orders are what it did, and a sell (if any)
                 still comes back as a round trip below them. -->
            {#if dca}
              <DcaFills events={dca.events ?? []} total={dca.events_total} tall />
            {/if}
            {#if !dca || result.trades.length}
              <TradesTable trades={result.trades} exitReasons={result.stats.exit_reasons}
                onpick={focusTrade} picked={pickedTrade} />
            {/if}
          {/if}
          </div>
        </div>
      </div>
    {:else}
      <div class="steps" role="tablist" aria-label={$t('backtest.step.aria')}>
        {#each STEPS as s, i (s.id)}
          <button
            role="tab"
            aria-selected={step === s.id}
            tabindex={step === s.id ? 0 : -1}
            class:on={step === s.id}
            onclick={() => (step = s.id)}
            onkeydown={(e) => onStepKey(e, i)}
            title={s.summary}
          >
            <span class="n" class:todo={!s.ok}>{i + 1}</span>
            <span class="lbl"><Icon name={s.icon} size={12} /> {s.label}</span>
          </button>
        {/each}
      </div>

      <div class="step-body">
        {#if step === 'data'}
          <DataStep {datasets} bind:datasetIds {alignment} {aligning} />
        {:else if step === 'strategy'}
          <StrategyStep
            bind:settings
            {customIndicators}
            assets={pickedAssets}
            timeframe={pickedTimeframe}
            onindicators={() => (indicatorsOpen = true)} />
        {:else if step === 'filters'}
          <FiltersStep bind:settings timeframe={pickedTimeframe} />
        {:else if step === 'sizing'}
          <SizingStep bind:settings {multi} />
        {:else}
          <AdvancedStep bind:settings />
        {/if}
      </div>

      <div class="step-foot">
        <button class="nav" disabled={stepIndex === 0} onclick={() => goStep(-1)}>
          <Icon name="chevron-left" size={13} /> {$t('backtest.step.prev')}
        </button>
        <button class="nav" disabled={stepIndex === STEPS.length - 1} onclick={() => goStep(1)}>
          {$t('backtest.step.next')} <Icon name="chevron-right" size={13} />
        </button>
        <span class="progress">{$t('backtest.step.of', { n: stepIndex + 1, total: STEPS.length })}</span>
        <span class="gate" class:ready={canRun}>
          {#if !datasetIds.length}
            {$t('backtest.step.needData')}
          {:else if stopMissing}
            {$t('backtest.settings.riskNeedsStop')}
          {:else}
            {$t('backtest.step.ready')}
          {/if}
        </span>
        <!-- Called through a lambda: as a handler it would take the click event for its
             replay argument, and a real run would record nothing. -->
        <button class="run" class:busy={running} disabled={!canRun || running} onclick={() => run()}>
          <!-- The fill is the loader: no number, no promise, just the work crossing the button. -->
          <span class="fill" style:transform="scaleX({runProgress})" aria-hidden="true"></span>
          <span class="lbl">
            {#if running}
              <span class="spin"><Icon name="refresh-cw" size={13} /></span>
              {$t('backtest.settings.running')}
            {:else}
              <Icon name="play" size={13} /> {$t('backtest.settings.runBacktest')}
            {/if}
          </span>
        </button>
      </div>
    {/if}
  </section>

  <!-- ── Dock: history + strategy library. Hidden while a result is open. ── -->
  {#if !(showResult && shown)}
  <section
    class="dock"
    class:shut={!dockOpen}
    class:dragging={!!dockDrag}
    bind:this={dockEl}
    style:flex={dockOpen ? (dockH ? `0 0 ${dockH}px` : '0 0 35%') : '0 0 auto'}
  >
    <!-- The dock's top edge: slid to size it, clicked to shut it. -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="dock-grip"
      bind:this={dockGrip}
      role="separator"
      tabindex="0"
      aria-orientation="horizontal"
      aria-label={$t('backtest.dock.resize')}
      title={$t('backtest.dock.resize')}
      onpointerdown={onDockDown}
      onpointermove={onDockMove}
      onpointerup={onDockUp}
      onpointercancel={onDockUp}
      onkeydown={onDockKey}
    ></div>
    <div class="dock-bar">
      <div class="tabs" role="tablist" aria-label={$t('backtest.dock.aria')}>
        <button role="tab" aria-selected={dockTab === 'history'} onclick={() => { dockTab = 'history'; if (!dockOpen) toggleDock(); }}>
          {$t('backtest.page.history')}{#if runs.length} <span class="count">{runs.length}</span>{/if}
        </button>
        <button role="tab" aria-selected={dockTab === 'strategies'} onclick={() => { dockTab = 'strategies'; if (!dockOpen) toggleDock(); }}>
          {$t('backtest.dock.strategies')}{#if strategies.length} <span class="count">{strategies.length}</span>{/if}
        </button>
        <button role="tab" aria-selected={dockTab === 'paper'} onclick={() => { dockTab = 'paper'; if (!dockOpen) toggleDock(); }}>
          {$t('backtest.paper.tab')}{#if paperSessions.length} <span class="count">{paperSessions.length}</span>{/if}{#if paperDot} <span class="live {paperDot}" title={$t(`backtest.paper.status.${paperDot === 'err' ? 'error' : 'active'}`)}></span>{/if}
        </button>
      </div>
      <div class="dock-acts">
        {#if dockTab === 'strategies'}
          <div class="search">
            <Icon name="search" size={12} />
            <input bind:value={dockQuery} placeholder={$t('backtest.dock.search')} />
            {#if dockQuery}
              <button class="clear" onclick={() => (dockQuery = '')} aria-label={$t('common.clear')}><Icon name="x" size={11} /></button>
            {/if}
          </div>
          <button class="sm" onclick={newStrategy}><Icon name="plus" size={12} /> {$t('backtest.dock.newStrategy')}</button>
        {:else if dockTab === 'paper'}
          {#if paperUnseen.length}
            <button class="sm" onclick={ackPaper}>
              {$t('backtest.paper.ackAll', { n: paperUnseen.length })}
            </button>
          {/if}
        {:else}
          <span class="hint">{$t('backtest.page.historyHint')}</span>
          {#if runs.length}
            <button class="sm" onclick={() => (clearHistoryOpen = true)}>{$t('backtest.page.clearHistory')}</button>
          {/if}
        {/if}
        <button
          class="dock-toggle"
          onclick={toggleDock}
          title={dockOpen ? $t('backtest.dock.collapse') : $t('backtest.dock.expand')}
          aria-label={dockOpen ? $t('backtest.dock.collapse') : $t('backtest.dock.expand')}
          aria-expanded={dockOpen}
        >
          <Icon name={dockOpen ? 'chevron-down' : 'chevron-up'} size={13} />
        </button>
      </div>
    </div>
    {#if dockOpen}
    <div class="dock-body">
      {#if dockTab === 'history'}
        <HistoryPanel {runs} activeId={activeRunId} onload={loadRun} onreport={openReport} onremove={removeRun} />
      {:else if dockTab === 'paper'}
        <PaperPanel
          sessions={paperSessions}
          unseen={paperUnseen}
          onedit={editPaper}
          onchanged={() => loadPaper().catch(() => {})}
        />
      {:else}
        <StrategyLibrary {strategies} query={dockQuery} activeId={strategyId}
          onopen={openStrategy} onduplicate={duplicateStrategy} onremove={deleteStrategy} />
      {/if}
    </div>
    {/if}
  </section>
  {/if}
</div>

<ConfirmModal
  bind:open={discardOpen}
  title={$t('backtest.header.discardTitle')}
  message={$t('backtest.header.discardConfirm')}
  confirmLabel={$t('backtest.header.discardYes')}
  cancelLabel={$t('common.cancel')}
  danger
  onconfirm={() => answerDiscard(true)}
  oncancel={() => answerDiscard(false)}
/>

<ConfirmModal
  bind:open={clearHistoryOpen}
  title={$t('backtest.page.clearHistory')}
  message={$t('backtest.page.clearHistoryConfirm')}
  confirmLabel={$t('backtest.page.clearHistory')}
  cancelLabel={$t('common.cancel')}
  danger
  onconfirm={clearHistory}
/>

<PaperModal
  bind:open={paperOpen}
  bind:session={paperDraft}
  tickers={paperTickers}
  onsaved={() => loadPaper().catch(() => {})}
  oneditstrategy={editPaperStrategy}
/>

<!-- The strategy a session froze has just been rewritten. The session either follows it or
     stays as it ran, with the new wording starting its own session. -->
<Modal bind:open={applyOpen} title={$t('backtest.paper.apply.title')} size="sm">
  <p class="apply-lead">{$t('backtest.paper.apply.lead', { name: paperOrigin?.session?.name ?? '' })}</p>
  <label class="save-field">
    <span>{$t('backtest.paper.apply.name')}</span>
    <input bind:value={applyName} />
  </label>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => { applyOpen = false; paperOrigin = null; }}>
      {$t('backtest.paper.apply.skip')}
    </Button>
    <Button variant="secondary" disabled={applyBusy} onclick={() => applyToSession('copy')}>
      {$t('backtest.paper.apply.copy')}
    </Button>
    <Button variant="primary" disabled={applyBusy} onclick={() => applyToSession('update')}>
      {$t('backtest.paper.apply.update')}
    </Button>
  {/snippet}
</Modal>

<SaveStrategyModal
  bind:open={strategyModalOpen}
  initialName={strategyName}
  initialTags={strategyTags}
  {allTags}
  error={strategyError}
  kind={settings.kind}
  existing={!!strategyId && !!strategyName.trim()}
  onsave={commitStrategy}
/>

<IndicatorLibraryModal bind:open={indicatorsOpen} {indicators} onchange={loadIndicators} />

<OptimizeModal
  bind:open={optimizeOpen}
  settings={optSettings}
  mirrored={settings.reverse_side && settings.mode !== 'long'}
  {datasetIds}
  onstart={(jobId) => goto(`/backtest/optimize/${jobId}`)}
/>

<!-- The chart's own indicator picker (same one the visualization module uses): what it adds is
     drawn over the result, never written back into the strategy. -->
<IndicatorModal bind:open={indicatorOpen} edit={editingInstance} onsave={saveInstance} onclose={() => (editingInstance = null)} />

<Modal bind:open={saveRunOpen} title={$t('backtest.page.saveResults')}>
  <label class="save-field">
    <span>{$t('backtest.page.name')}</span>
    <!-- svelte-ignore a11y_autofocus -->
    <input bind:value={saveRunName} autofocus placeholder={$t('backtest.page.namePlaceholder')}
      onkeydown={(e) => e.key === 'Enter' && saveRun()} />
  </label>
  {#snippet footer()}
    <button class="ghost" onclick={() => (saveRunOpen = false)}>{$t('common.cancel')}</button>
    <button class="primary" onclick={saveRun}>{$t('common.save')}</button>
  {/snippet}
</Modal>
</RequireModule>

<!-- The session is written on the way out of the document (a reload, a closed tab), never on
     an in-app navigation, which is what tells the two apart. -->
<svelte:window onpagehide={onUnload} onbeforeunload={onUnload} />

<style>
  .page {
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: var(--space-4);
    gap: var(--space-3);
    overflow: hidden;
  }

  /* ── Header: title, strategy identity, library actions ── */
  .top {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  h1 {
    font-size: 1.25rem;
    font-weight: var(--fw-medium);
    letter-spacing: -0.01em;
  }
  .ident {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
    flex: 1;
  }
  .name {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    background: var(--surface-2);
    border: var(--hairline) solid var(--border);
    border-radius: 0;
    color: var(--text);
    font-size: var(--text-sm);
    height: var(--control-h);
    padding: 0 var(--space-3);
    cursor: pointer;
    max-width: 340px;
    overflow: hidden;
  }
  .name span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .name:hover {
    border-color: var(--border-control);
  }
  .untitled {
    color: var(--muted);
    font-style: italic;
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--amber);
    flex-shrink: 0;
  }
  .tag {
    font-size: 0.68rem;
    color: var(--muted);
    border: var(--hairline) solid var(--border);
    padding: 1px var(--space-2);
    white-space: nowrap;
  }
  .acts {
    display: flex;
    gap: var(--space-2);
    margin-left: auto;
  }
  .top button,
  .head-acts button,
  .dock-acts .sm {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: var(--surface-2);
    border: var(--hairline) solid var(--border-control);
    border-radius: 0;
    color: var(--text);
    font-size: var(--text-sm);
    height: var(--control-h);
    padding: 0 var(--space-3);
    cursor: pointer;
    white-space: nowrap;
  }
  .top button:hover,
  .head-acts button:hover,
  .dock-acts .sm:hover {
    background: color-mix(in srgb, var(--accent) 12%, var(--surface-2));
  }
  .top .primary,
  .head-acts .primary {
    background: color-mix(in srgb, var(--accent) 22%, var(--surface-2));
  }
  .err {
    color: var(--red);
    font-size: var(--text-sm);
    cursor: pointer;
  }
  /* A sweep still running: the dot is the only moving thing in the header, which is the point. */
  .top .opt-live {
    border-color: var(--accent);
  }
  .pulse {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent);
    animation: opt-pulse 1.6s ease-in-out infinite;
  }
  @keyframes opt-pulse {
    50% {
      opacity: 0.25;
    }
  }

  /* ── Work area ── */
  .work {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    border: var(--hairline) solid var(--border);
    background: var(--surface);
    overflow: hidden;
  }

  /* Stepper */
  /* One compact rail: number, icon, name. The step's detail is a hover title, not a second
     line — the strip is a place in the flow, and it must stay on one row at any step count. */
  .steps {
    display: flex;
    align-items: stretch;
    border-bottom: var(--hairline) solid var(--border);
    flex-shrink: 0;
    overflow-x: auto;
    scrollbar-width: none;
  }
  .steps::-webkit-scrollbar {
    display: none;
  }
  .steps button {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    padding: var(--space-2) var(--space-4);
    color: var(--muted);
    white-space: nowrap;
    cursor: pointer;
    transition: color var(--dur-fast) var(--ease);
  }
  /* A hairline tick between steps, not a full divider: the row reads as one rail. */
  .steps button:not(:last-child)::after {
    content: '';
    width: 1px;
    height: 12px;
    margin-left: var(--space-3);
    background: var(--border);
  }
  .steps button:hover {
    color: var(--text);
  }
  .steps button.on {
    color: var(--text);
    border-bottom-color: var(--accent);
  }
  .steps .n {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    flex-shrink: 0;
    border: var(--hairline) solid var(--border-control);
    font-size: 0.66rem;
    font-variant-numeric: tabular-nums;
    color: var(--muted);
  }
  .steps button.on .n {
    background: color-mix(in srgb, var(--accent) 22%, transparent);
    border-color: var(--accent);
    color: var(--text);
  }
  .steps .n.todo {
    border-color: var(--amber);
    color: var(--amber);
  }
  .lbl {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    color: inherit;
  }

  .step-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: var(--space-4);
  }

  .step-foot {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-4);
    border-top: var(--hairline) solid var(--border);
    background: var(--surface-2);
  }
  /* Same weight as Run backtest: the three are one control row. */
  .nav {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    background: transparent;
    border: var(--hairline) solid var(--border-control);
    border-radius: 0;
    color: var(--text);
    font-size: 0.9rem;
    font-weight: var(--fw-medium);
    padding: var(--space-2) var(--space-4);
    cursor: pointer;
  }
  .nav:hover:not(:disabled) {
    background: var(--surface);
  }
  .nav:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .progress {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .gate {
    margin-left: auto;
    font-size: var(--text-xs);
    color: var(--amber);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .gate.ready {
    color: var(--green);
  }
  .run {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    background: color-mix(in srgb, var(--accent) 22%, var(--surface));
    border: var(--hairline) solid var(--border-control);
    border-radius: 0;
    padding: var(--space-2) var(--space-6);
    color: var(--text);
    font-weight: var(--fw-medium);
    font-size: 0.9rem;
    cursor: pointer;
    white-space: nowrap;
  }
  .run:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent) 34%, var(--surface));
  }
  .run:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
  /* Running: the button keeps its weight and reads the work instead. */
  .run.busy {
    opacity: 1;
    position: relative;
    overflow: hidden;
    cursor: progress;
  }
  .run .fill {
    position: absolute;
    inset: 0;
    transform-origin: left center;
    transform: scaleX(0);
    background: color-mix(in srgb, var(--accent) 55%, transparent);
    /* Slightly behind the tick that sets it, so the edge moves instead of stepping. */
    transition: transform 140ms linear;
    pointer-events: none;
  }
  .run .lbl {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
  }
  .spin {
    display: inline-flex;
    animation: run-spin 1.1s linear infinite;
  }
  @keyframes run-spin {
    to {
      transform: rotate(360deg);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .spin {
      animation: none;
    }
    .run .fill {
      transition: none;
    }
  }

  /* Result view */
  .result-head {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-4);
    border-bottom: var(--hairline) solid var(--border);
    flex-shrink: 0;
    flex-wrap: wrap;
  }
  .back {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: transparent;
    border: var(--hairline) solid var(--border-control);
    border-radius: 0;
    color: var(--text);
    font-size: var(--text-sm);
    /* One control height across the row: the button, the asset picker and the head actions
       all stand on --control-h, so none of them is a hair shorter than its neighbour. */
    height: var(--control-h);
    padding: 0 var(--space-3);
    cursor: pointer;
  }
  .back:hover {
    background: var(--surface-2);
  }
  .result-head .title {
    font-weight: var(--fw-medium);
  }
  /* The timeframe stands in the row of controls, so it stands at their height too. */
  .result-head .chip {
    display: inline-flex;
    align-items: center;
    height: var(--control-h);
    font-size: var(--text-xs);
    color: var(--muted);
    border: var(--hairline) solid var(--border-control);
    padding: 0 var(--space-3);
    white-space: nowrap;
  }
  .result-head .sub {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .head-acts {
    margin-left: auto;
    display: flex;
    align-items: stretch;
    gap: var(--space-2);
  }
  .head-acts button {
    height: var(--control-h);
  }
  /* The reports menu: one header slot, its two ways out listed under it. */
  .menu-wrap {
    position: relative;
    display: flex;
  }
  .menu-wrap .menu {
    position: absolute;
    z-index: var(--z-dropdown, 50);
    top: calc(100% + 2px);
    right: 0;
    min-width: 100%;
    display: flex;
    flex-direction: column;
    padding: var(--space-1);
    background: var(--surface);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    box-shadow: var(--shadow-lg, 0 8px 24px rgb(0 0 0 / 0.35));
  }
  /* Rows inside a framed menu: no frame of their own. */
  .menu-wrap .menu button {
    justify-content: flex-start;
    background: transparent;
    border: 0;
    color: var(--muted);
    width: 100%;
  }
  .menu-wrap .menu button:hover {
    background: var(--surface-2);
    color: var(--text);
  }
  /* Narrow: it names one instrument, and the header carries the rest of the run. */
  .pick.asset {
    width: 150px;
    flex: none;
  }
  /* The picker, then that asset's headline figures on the same line. */
  .asset-bar {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    flex-wrap: wrap;
  }
  .asset-bar .sub {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .asset-bar .sub b {
    color: var(--text);
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  .asset-bar .pos {
    color: var(--green);
  }
  .asset-bar .neg {
    color: var(--red);
  }
  /* Chart over results: the chart keeps the height the user dragged it to, the results take
     the rest and scroll on their own. */
  .split {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .chart-pane {
    position: relative;
    display: flex;
    flex-shrink: 0;
    min-height: 0;
    padding: var(--space-2) var(--space-2) 0;
    /* The chart is a canvas that keeps its own size between frames: without this it paints
       over the statistics for as long as the drag lasts. */
    overflow: hidden;
  }
  /* The chart page gives the chart a 320px floor; here the pane's height is the user's, and a
     floor is exactly what would let it spill. */
  .chart-pane :global(.chart-host),
  .chart-pane :global(.chart) {
    min-height: 0;
  }
  .chart-pane :global(.chart-host) {
    flex: 1;
    min-width: 0;
  }
  .chart-pane.fullscreen {
    position: fixed;
    inset: 0;
    z-index: var(--z-modal);
    background: var(--surface);
    padding: var(--space-3);
  }
  .fs-close {
    position: absolute;
    top: var(--space-3);
    right: var(--space-3);
    z-index: 1;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    background: var(--surface-2);
    border: var(--hairline) solid var(--border);
    color: var(--muted);
    cursor: pointer;
  }
  .fs-close:hover {
    color: var(--text);
  }
  /* The grab bar. Thin, but with a real hit area and a visible grip on hover. */
  .grip {
    flex: none;
    height: 9px;
    cursor: row-resize;
    position: relative;
  }
  /* Shut on one side, the whole bar is the way back: one strip, one hairline, no button
     framed inside it. The hairline faces the pane that is still there. */
  .grip.banner {
    height: 30px;
    display: flex;
    background: var(--surface-2);
    border-top: var(--hairline) solid var(--border);
  }
  .grip.banner.up {
    border-top: 0;
    border-bottom: var(--hairline) solid var(--border);
  }
  .grip.banner::after {
    display: none;
  }
  /* The strip itself, so it can still be dragged: the click only restores when the pointer
     did not travel. */
  .restore {
    flex: 1;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    background: transparent;
    border: 0;
    border-radius: 0;
    color: var(--muted);
    font-size: var(--text-xs);
    font-family: inherit;
    letter-spacing: 0.02em;
    padding: 0;
    cursor: row-resize;
  }
  .restore :global(svg) {
    opacity: 0.8;
  }
  .grip.banner:hover .restore {
    color: var(--text);
    background: color-mix(in srgb, var(--accent) 8%, transparent);
  }
  .grip::after {
    content: '';
    position: absolute;
    left: 50%;
    top: 4px;
    width: 46px;
    height: 1px;
    transform: translateX(-50%);
    background: var(--accent);
  }
  .grip:hover::after,
  .grip:focus-visible::after,
  .grip.dragging::after {
    background: var(--accent);
    height: 2px;
  }
  .grip:focus-visible {
    outline: none;
  }
  .result-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }
  .result-inner {
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  /* Standing in for a tab whose content is being recomputed. */
  .sk-body {
    min-height: 320px;
  }
  .restoring {
    color: var(--amber);
  }
  .empty-note {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--muted);
  }
  /* The performance chart owns its pane: equity, excursions and the drawdown strip. */
  .curve-box {
    height: 460px;
    min-height: 0;
  }
  .badges {
    display: flex;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .badge {
    font-size: var(--text-xs);
    color: var(--muted);
    border: var(--hairline) solid var(--border);
    padding: 2px var(--space-2);
  }
  .badge.warn {
    color: var(--amber);
    border-color: var(--amber);
  }
  .grid-stats {
    display: flex;
    gap: var(--space-4);
    font-size: var(--text-sm);
    color: var(--muted);
    flex-wrap: wrap;
  }
  .grid-stats .cap {
    text-transform: uppercase;
    letter-spacing: 0.08em;
    font-size: var(--text-xs);
  }
  .count {
    color: var(--muted);
    font-size: var(--text-xs);
    margin-left: 4px;
  }
  /* Paper: running or broken, as a state and not as a number. */
  .live {
    display: inline-block;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    margin-left: 4px;
    vertical-align: middle;
  }
  .live.ok {
    background: var(--green);
  }
  .live.err {
    background: var(--red);
  }

  /* ── Dock ── */
  .dock {
    flex: 0 0 35%;
    min-height: 0;
    display: flex;
    flex-direction: column;
    border: var(--hairline) solid var(--border);
    background: var(--surface);
    overflow: hidden;
    transition: flex-basis 140ms ease;
  }
  /* The drag has to follow the pointer, not an easing curve. */
  .dock.dragging {
    transition: none;
  }
  .dock.shut {
    flex: 0 0 auto;
  }
  /* Same grab bar as the chart split, on the dock's top edge. */
  .dock-grip {
    flex: none;
    height: 9px;
    cursor: row-resize;
    position: relative;
  }
  .dock-grip::after {
    content: '';
    position: absolute;
    left: 50%;
    top: 4px;
    width: 46px;
    height: 1px;
    transform: translateX(-50%);
    background: var(--border-control);
  }
  .dock-grip:hover::after,
  .dock-grip:focus-visible::after,
  .dock.dragging .dock-grip::after {
    background: var(--accent);
    height: 2px;
  }
  .dock-grip:focus-visible {
    outline: none;
  }
  .dock-toggle {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    height: var(--control-h);
    width: var(--control-h);
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: 0;
  }
  .dock-toggle:hover {
    color: var(--text);
  }
  .dock-bar {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: 0 var(--space-3);
    border-bottom: var(--hairline) solid var(--border);
    flex-shrink: 0;
  }
  .dock-bar .tabs {
    border-bottom: none;
  }
  .dock-acts {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }
  .dock-acts .hint {
    font-size: var(--text-xs);
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .search {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    border: var(--hairline) solid var(--border-control);
    background: var(--surface-2);
    height: var(--control-h);
    padding: 0 var(--space-2);
    color: var(--muted);
  }
  .search input {
    border: none;
    background: transparent;
    color: var(--text);
    font-size: var(--text-sm);
    outline: none;
    width: 190px;
  }
  .clear {
    display: inline-flex;
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: 0;
  }
  .dock-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }

  .apply-lead {
    margin: 0 0 var(--space-3);
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .save-field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .ghost,
  .primary {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: var(--surface-2);
    border: var(--hairline) solid var(--border-control);
    border-radius: 0;
    color: var(--text);
    font-size: var(--text-sm);
    padding: var(--space-1) var(--space-3);
    cursor: pointer;
  }
  .primary {
    background: color-mix(in srgb, var(--accent) 22%, var(--surface-2));
  }

</style>
