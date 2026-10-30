<script>
  // Import a list into the universe.
  //
  // - A chart list: its rows already carry resolved coordinates (provider, symbol,
  //   timeframe, connector), so they become items as they are.
  // - A watchlist: its rows are quote symbols, so each one is looked up on the connectors
  //   granted to the Scanner and taken only on an exact symbol match on one connector (the
  //   row's own connector when it has one). Anything else is refused with the reason, never
  //   matched to the nearest guess.
  //
  // A row already in the universe is counted, not duplicated.
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { scannerApi } from './api.js';
  import { watchlistsApi } from '$lib/modules/watchlists/api.js';
  import { t } from '$lib/i18n';

  let { open = $bindable(false), hives = [], onchanged } = $props();

  const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
  const TIMEFRAMES = ['1m', '5m', '15m', '30m', '1h', '4h', '1d', '1w'];

  let source = $state('chart');
  let lists = $state([]);
  let watchlists = $state([]);
  let listId = $state('');
  let watchId = $state('');
  let watchRows = $state([]);
  let timeframe = $state('1d');
  let hiveId = $state('');
  let busy = $state(false);
  let error = $state('');
  let report = $state(null);
  let progress = $state('');

  $effect(() => {
    if (!open) return;
    report = null;
    error = '';
    scannerApi
      .chartLists()
      .then((rows) => {
        lists = rows;
        if (!listId && rows.length) listId = rows[0].id;
      })
      .catch((e) => (error = e.message));
    watchlistsApi
      .list()
      .then((rows) => {
        watchlists = rows;
        if (!watchId && rows.length) watchId = rows[0].id;
      })
      .catch(() => {
        /* the Watchlists module may be off: chart lists still import */
      });
  });

  $effect(() => {
    const id = watchId;
    if (!open || source !== 'watch' || !id) return;
    watchlistsApi
      .detail(id)
      .then((r) => (watchRows = r.items ?? []))
      .catch((e) => (error = e.message));
  });

  const picked = $derived(lists.find((l) => l.id === listId));
  const chartRows = $derived(Array.isArray(picked?.items) ? picked.items : []);
  const count = $derived(source === 'chart' ? chartRows.length : watchRows.length);

  async function add(body, label, out) {
    try {
      await scannerApi.createItem({ ...body, hive_ids: hiveId ? [hiveId] : [] });
      out.added += 1;
    } catch (e) {
      if (/already an item/.test(e.message)) out.already += 1;
      else out.refused.push(`${label}: ${e.message}`);
    }
  }

  /** One watchlist row resolved on the Scanner's connectors, or why it is not. */
  async function resolve(row) {
    const sym = (row.quote_ticker || row.symbol || '').trim();
    if (!sym) return { why: $t('scanner.import.noSymbol') };
    const r = await scannerApi.symbols({ q: sym, limit: 10 });
    let exact = (r.results ?? []).filter((h) => h.symbol.toUpperCase() === sym.toUpperCase());
    if (UUID.test(row.quote_source ?? '')) exact = exact.filter((h) => h.connector_id === row.quote_source);
    const connectors = new Set(exact.map((h) => h.connector_id));
    if (!exact.length) return { why: $t('scanner.import.notFound', { symbol: sym }) };
    if (connectors.size > 1 || exact.length > 1) return { why: $t('scanner.import.ambiguous', { symbol: sym }) };
    return { hit: exact[0] };
  }

  async function run() {
    busy = true;
    error = '';
    const out = { added: 0, already: 0, refused: [] };
    if (source === 'chart') {
      for (const r of chartRows) {
        await add(
          {
            provider: r.provider,
            asset_type: r.asset_type,
            ticker: r.ticker,
            timeframe: r.timeframe,
            connector_id: r.connector_id ?? null,
            label: r.name ?? ''
          },
          `${r.ticker} ${r.timeframe}`,
          out
        );
      }
    } else {
      let i = 0;
      for (const row of watchRows) {
        progress = $t('scanner.import.progress', { i: ++i, n: watchRows.length });
        try {
          const { hit, why } = await resolve(row);
          if (!hit) {
            out.refused.push(`${row.symbol}: ${why}`);
            continue;
          }
          if (!hit.timeframes?.includes(timeframe)) {
            out.refused.push(`${row.symbol}: ${$t('scanner.add.noTimeframe', { tf: timeframe, connector: hit.connector })}`);
            continue;
          }
          await add(
            {
              provider: hit.provider,
              asset_type: hit.asset_type,
              ticker: hit.symbol,
              timeframe,
              connector_id: hit.connector_id,
              label: row.name ?? ''
            },
            row.symbol,
            out
          );
        } catch (e) {
          out.refused.push(`${row.symbol}: ${e.message}`);
        }
      }
      progress = '';
    }
    report = out;
    busy = false;
    onchanged?.();
  }
</script>

<Modal bind:open title={$t('scanner.import.title')} size="md">
  <div class="form">
    <div class="src" role="radiogroup" aria-label={$t('scanner.import.source')}>
      <button type="button" role="radio" aria-checked={source === 'chart'} class="chip" class:active={source === 'chart'} onclick={() => (source = 'chart')}>
        {$t('scanner.import.fromChart')}
      </button>
      <button type="button" role="radio" aria-checked={source === 'watch'} class="chip" class:active={source === 'watch'} onclick={() => (source = 'watch')}>
        {$t('scanner.import.fromWatchlist')}
      </button>
    </div>

    <div class="form-grid">
      {#if source === 'chart'}
        <label class="field">
          <span>{$t('scanner.import.list')}</span>
          {#if lists.length}
            <Dropdown bind:value={listId} float options={lists.map((l) => ({ value: l.id, label: `${l.name} (${(l.items ?? []).length})` }))} />
          {:else}
            <span class="hint">{$t('scanner.import.noLists')}</span>
          {/if}
        </label>
      {:else}
        <label class="field">
          <span>{$t('scanner.import.watchlist')}</span>
          {#if watchlists.length}
            <Dropdown bind:value={watchId} float options={watchlists.map((w) => ({ value: w.id, label: w.name }))} />
          {:else}
            <span class="hint">{$t('scanner.import.noWatchlists')}</span>
          {/if}
        </label>
        <label class="field">
          <span>{$t('scanner.timeframe')}</span>
          <Dropdown bind:value={timeframe} float options={TIMEFRAMES.map((v) => ({ value: v, label: v }))} />
        </label>
      {/if}
      <label class="field">
        <span>{$t('scanner.add.joinHive')}</span>
        <Dropdown
          bind:value={hiveId}
          float
          options={[{ value: '', label: $t('scanner.add.noHive') }, ...hives.map((h) => ({ value: h.id, label: h.name }))]}
        />
      </label>
    </div>
    <p class="hint">
      {source === 'chart' ? $t('scanner.import.help', { n: count }) : $t('scanner.import.watchHelp', { n: count })}
    </p>
    {#if progress}<p class="hint">{progress}</p>{/if}
    {#if error}<ErrorText {error} />{/if}
    {#if report}
      <p class="ok">{$t('scanner.import.report', { added: report.added, already: report.already, refused: report.refused.length })}</p>
      <ul class="refused">
        {#each report.refused as r (r)}
          <li>{r}</li>
        {/each}
      </ul>
    {/if}
  </div>
  {#snippet footer()}
    <Button onclick={() => (open = false)}>{$t('scanner.close')}</Button>
    <Button variant="primary" icon="download" loading={busy} disabled={!count} onclick={run}>
      {$t('scanner.import.run')}
    </Button>
  {/snippet}
</Modal>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .src {
    display: flex;
    gap: var(--space-2);
  }
  .ok {
    font-size: var(--text-sm);
  }
  .refused {
    list-style: none;
    margin: 0;
    padding: 0;
    color: var(--dim);
    font-size: var(--text-xs);
    max-height: 200px;
    overflow-y: auto;
  }
</style>
