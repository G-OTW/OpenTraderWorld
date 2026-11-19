<script>
  // The hit log: every (alert, item, closed bar) recorded, newest first, with what it read,
  // and the chart one click away.
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { scannerApi, chartUrl } from './api.js';
  import { fmtNum } from '$lib/format.js';
  import { t } from '$lib/i18n';

  let { alerts = [], hives = [], alertId = null } = $props();

  let hits = $state([]);
  let loading = $state(true);
  let error = $state('');
  // Seeded once from the notification's link; the filter is the user's afterwards.
  // svelte-ignore state_referenced_locally
  let alertFilter = $state(alertId ?? '');
  let hiveFilter = $state('');

  $effect(() => {
    const q = { alert_id: alertFilter || null, hive_id: hiveFilter || null, limit: 300 };
    loading = true;
    scannerApi
      .hits(q)
      .then((rows) => {
        hits = rows;
        error = '';
      })
      .catch((e) => (error = e.message))
      .finally(() => (loading = false));
  });

  function values(v) {
    return Object.entries(v ?? {})
      .filter(([, x]) => typeof x === 'number')
      .map(([k, x]) => `${k} ${fmtNum(x, 4)}`)
      .join(' · ');
  }
</script>

<div class="wrap">
  <div class="filter-bar">
    <div class="dd">
      <Dropdown
        bind:value={alertFilter}
        ariaLabel={$t('scanner.hits.alert')}
        options={[{ value: '', label: $t('scanner.hits.allAlerts') }, ...alerts.map((a) => ({ value: a.id, label: a.name || $t('scanner.alerts.unnamed') }))]}
      />
    </div>
    <div class="dd">
      <Dropdown
        bind:value={hiveFilter}
        ariaLabel={$t('scanner.hits.hive')}
        options={[{ value: '', label: $t('scanner.hits.allHives') }, ...hives.map((h) => ({ value: h.id, label: h.name }))]}
      />
    </div>
  </div>
  {#if error}<ErrorText {error} />{/if}

  {#if loading && !hits.length}
    <Skeleton />
  {:else if !hits.length}
    <EmptyState icon="history" title={$t('scanner.hits.emptyTitle')} description={$t('scanner.hits.emptyBody')} />
  {:else}
    <div class="scroll">
      <table class="tbl">
        <thead>
          <tr>
            <th>{$t('scanner.hits.when')}</th>
            <th>{$t('scanner.hits.alert')}</th>
            <th>{$t('scanner.items.instrument')}</th>
            <th>{$t('scanner.col.bar')}</th>
            <th>{$t('scanner.hits.values')}</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each hits as h (h.id)}
            <tr>
              <td class="dim when" data-label={$t('scanner.hits.when')}>{new Date(h.created_at).toLocaleString()}</td>
              <td class="al" data-label={$t('scanner.hits.alert')}>{h.alert_name}</td>
              <td class="inst"><span class="sym">{h.ticker}</span> <span class="dim">{h.timeframe}</span></td>
              <td class="dim bar" data-label={$t('scanner.col.bar')}>{new Date(h.bar_ts).toLocaleString()}</td>
              <td class="dim vals" data-label={$t('scanner.hits.values')}>{values(h.values)}</td>
              <td class="go"><a href={chartUrl(h)}>{$t('scanner.hits.open')}</a></td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</div>

<style>
  .wrap {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .scroll {
    overflow-x: auto;
  }
  .sym {
    font-family: var(--mono);
  }
  .dim {
    color: var(--dim);
    font-size: var(--text-xs);
  }
  /* Phone: the filters take a line each and each hit is a card, instrument and chart link on
     top, the other columns captioned below. The header row stays for screen readers only. */
  @media (max-width: 767px) {
    .filter-bar > .dd {
      flex: 1 1 100%;
      min-width: 0;
    }
    .filter-bar :global(.trigger) {
      min-height: 44px;
    }
    .scroll .tbl,
    .scroll tbody {
      display: block;
    }
    .scroll thead {
      position: absolute;
      width: 1px;
      height: 1px;
      overflow: hidden;
      clip-path: inset(50%);
      white-space: nowrap;
    }
    .scroll tbody tr {
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: var(--space-2) var(--space-3);
      padding: var(--space-3) 0;
      border-bottom: var(--hairline) solid var(--border);
    }
    .scroll .tbl td {
      height: auto;
      min-width: 0;
      padding: 0;
      border: none;
      overflow-wrap: anywhere;
    }
    .scroll .tbl td[data-label]::before {
      content: attr(data-label);
      display: block;
      font-size: 10px;
      text-transform: uppercase;
      letter-spacing: 0.06em;
      color: var(--dim);
    }
    td.inst {
      grid-column: 1;
      grid-row: 1;
      align-self: center;
      font-size: var(--text-base);
    }
    td.go {
      grid-column: 2;
      grid-row: 1;
      text-align: right;
    }
    td.go a {
      display: inline-flex;
      align-items: center;
      min-height: 44px;
    }
    td.al,
    td.vals {
      grid-column: 1 / -1;
    }
  }
</style>
