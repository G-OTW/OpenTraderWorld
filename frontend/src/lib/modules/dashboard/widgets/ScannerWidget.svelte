<script>
  // Scanner widgets: one hive, drawn by the board's own views. A honeycomb colored by a
  // measure, small line charts of the recent closes, or a table whose columns the user
  // picks. Values are the evaluator's last closed bar (the items' cache), re-read every
  // minute; the widget never goes live, the board does.
  //
  // Config: { variant, hive_id, metric, sort, size, hitsOnly, columns, limit }.
  // No hive_id means the first hive; a hive_id that no longer exists says so rather than
  // showing another hive.
  import { untrack } from 'svelte';
  import { scannerApi } from '$lib/modules/scanner/api.js';
  import { DEFAULT_GRID_COLUMNS, SIZES } from '$lib/modules/scanner/board.js';
  import HoneycombView from '$lib/modules/scanner/HoneycombView.svelte';
  import LinesView from '$lib/modules/scanner/LinesView.svelte';
  import GridView from '$lib/modules/scanner/GridView.svelte';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import { t } from '$lib/i18n';
  import WidgetState from './WidgetState.svelte';

  let { item, editing } = $props();

  /** Bar-close values refresh this often, as on the board. */
  const REFRESH_MS = 60_000;

  const variant = $derived(item.config?.variant ?? 'honeycomb');
  const hiveId = $derived(item.config?.hive_id ?? null);
  const metric = $derived(item.config?.metric ?? 'change_pct');
  const size = $derived(SIZES.includes(item.config?.size) ? item.config.size : 'm');
  const hitsOnly = $derived(item.config?.hitsOnly === true);
  const columns = $derived(Array.isArray(item.config?.columns) ? item.config.columns : DEFAULT_GRID_COLUMNS);
  const limit = $derived(Math.max(1, Math.min(200, item.config?.limit ?? 20)));

  // A header click re-sorts the table for this look only; the saved order is the config's,
  // and saving the config puts it back.
  let sortOverride = $state(null);
  const sort = $derived(sortOverride ?? item.config?.sort ?? 'ticker');
  $effect(() => {
    void item.config?.sort;
    sortOverride = null;
  });

  const palette = $derived(chartColors());

  let hives = $state(null);
  let signals = $state([]);
  let board = $state(null);
  let hits = $state([]);
  let err = $state('');

  const hive = $derived(hiveId ? (hives?.find((h) => h.id === hiveId) ?? null) : (hives?.[0] ?? null));
  const missing = $derived(!!hiveId && hives !== null && !hive);

  async function load(id, kind) {
    try {
      const [h, s] = await Promise.all([scannerApi.hives(), scannerApi.signals()]);
      const target = id ? h.hives.find((x) => x.id === id) : h.hives[0];
      const [b, hs] = target
        ? await Promise.all([
            scannerApi.board(target.id),
            kind === 'lines' ? scannerApi.hits({ hive_id: target.id, limit: 300 }) : []
          ])
        : [{ items: [] }, []];
      hives = h.hives;
      signals = s;
      board = b;
      hits = hs;
      err = '';
    } catch (e) {
      err = e.message;
    }
  }

  $effect(() => {
    if (editing) return;
    const id = hiveId;
    const kind = variant;
    board = null;
    untrack(() => load(id, kind));
    const timer = setInterval(() => untrack(() => load(id, kind)), REFRESH_MS);
    return () => clearInterval(timer);
  });
</script>

<WidgetState
  {editing}
  error={err}
  loading={board === null}
  empty={hives?.length === 0}
  preview={$t('dashboard.widgets.scanner.preview')}
  emptyText={$t('dashboard.widgets.scanner.empty')}
  emptyAction={$t('dashboard.widgets.scanner.emptyAction')}
  emptyHref="/scanner?tab=hives"
  rows={4}
>
  {#if missing}
    <p class="w-state">{$t('dashboard.widgets.scanner.hiveGone')}</p>
  {:else}
    <div class="scan">
      <span class="w-sub">{$t('dashboard.widgets.scanner.hiveItems', { name: hive?.name ?? '', count: board.items.length })}</span>
      {#if variant === 'lines'}
        <LinesView items={board.items} {hives} {hits} {palette} {sort} {size} {hitsOnly} />
      {:else if variant === 'table'}
        <GridView items={board.items} {hives} {signals} {sort} {hitsOnly} {columns} {limit} onsort={(s) => (sortOverride = s)} />
      {:else}
        <HoneycombView items={board.items} {hives} {signals} {palette} {metric} {sort} {size} {hitsOnly} />
      {/if}
    </div>
  {/if}
</WidgetState>

<style>
  .scan {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }
</style>
