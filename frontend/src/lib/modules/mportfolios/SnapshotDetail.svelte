<script>
  import Icon from '$lib/ui/Icon.svelte';
  import { copyLog } from '$lib/ui/copyLog.js';
  // Frozen holdings detail for one saved snapshot, shown in a wide modal. Mirrors MportfolioDetail
  // but loads by snapshot id from the immutable snapshot tables (no live Dataroma data, no 📷).
  import Modal from '$lib/ui/Modal.svelte';
  import { mportfoliosApi, fmtValue, fmtNum, fmtPrice, fmtPct } from './api.js';
  import { t } from '$lib/i18n';

  let { id = $bindable(null) } = $props();

  let snapshot = $state(null);
  let holdings = $state([]);
  let loading = $state(false);
  let error = $state('');

  let open = $derived(id != null);

  $effect(() => {
    if (!id) return;
    loading = true;
    error = '';
    snapshot = null;
    holdings = [];
    mportfoliosApi
      .snapshotDetail(id)
      .then((r) => {
        snapshot = r.snapshot;
        holdings = r.holdings;
      })
      .catch((e) => (error = e.message))
      .finally(() => (loading = false));
  });

  function close() {
    id = null;
  }

  let takenLabel = $derived(snapshot?.taken_at ? new Date(snapshot.taken_at).toLocaleString() : '');
</script>

<Modal {open} size="lg" title={snapshot?.name ?? $t('mportfolios.detail.snapshotTitle')} onclose={close}>
  {#if loading}
    <p class="muted">{$t('common.loading')}</p>
  {:else if error}
    <p class="err" title={$t('mportfolios.clickToCopy')} use:copyLog={error}>{error}</p>
  {:else if snapshot}
    <div class="meta">
      <span class="chip snap-chip"><Icon name="camera" size={12} /> {takenLabel}</span>
      {#if snapshot.period}<span class="chip">{snapshot.period}</span>{/if}
      <span class="chip">{fmtValue(snapshot.value_num, snapshot.value_text)}</span>
      <span class="chip">{$t('mportfolios.detail.holdingsCount', { count: snapshot.stock_count })}</span>
      <a class="src" href={snapshot.source_url} target="_blank" rel="noopener">{$t('mportfolios.detail.viewOn')} Dataroma <Icon name="external-link" size={11} /></a>
    </div>

    <div class="table-wrap">
      <table class="tbl">
        <thead>
          <tr>
            <th class="l">{$t('mportfolios.detail.colTicker')}</th>
            <th class="l">{$t('mportfolios.detail.colCompany')}</th>
            <th>{$t('mportfolios.detail.colPctPort')}</th>
            <th class="l">{$t('mportfolios.detail.colActivity')}</th>
            <th>{$t('mportfolios.detail.colShares')}</th>
            <th>{$t('mportfolios.detail.colReported')}</th>
            <th>{$t('mportfolios.detail.colValue')}</th>
            <th>{$t('mportfolios.detail.colCurrent')}</th>
            <th>{$t('mportfolios.detail.colChange')}</th>
            <th>{$t('mportfolios.detail.col52wLow')}</th>
            <th>{$t('mportfolios.detail.col52wHigh')}</th>
          </tr>
        </thead>
        <tbody>
          {#each holdings as h (h.position)}
            <tr>
              <td class="l tk">{h.ticker}</td>
              <td class="l">{h.company}</td>
              <td>{fmtPct(h.pct)}</td>
              <td class="l act" class:buy={/buy|add/i.test(h.activity)} class:sell={/sell|reduce/i.test(h.activity)}>
                {h.activity || '—'}
              </td>
              <td>{fmtNum(h.shares)}</td>
              <td>{fmtPrice(h.reported_price)}</td>
              <td>{fmtValue(h.value)}</td>
              <td>{fmtPrice(h.current_price)}</td>
              <td class:up={h.change_pct > 0} class:down={h.change_pct < 0}>{fmtPct(h.change_pct)}</td>
              <td>{fmtPrice(h.week52_low)}</td>
              <td>{fmtPrice(h.week52_high)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</Modal>

<style>
  .meta {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
    margin-bottom: var(--space-3);
  }
  .snap-chip {
    color: var(--text);
  }
  .src {
    margin-left: auto;
    color: var(--accent);
    font-size: 0.8rem;
    text-decoration: none;
  }
  .table-wrap {
    overflow-x: auto;
  }
  th,
  td {
    padding: var(--space-1) var(--space-2);
    text-align: right;
    border-bottom: 1px solid var(--border);
  }
  th {
    color: var(--muted);
    font-weight: 600;
    position: sticky;
    top: 0;
    background: var(--surface);
  }
  .l {
    text-align: left;
  }
  .tk {
    font-weight: 600;
    color: var(--text);
  }
  .act.buy {
    color: var(--green);
  }
  .act.sell {
    color: var(--red);
  }
  .up {
    color: var(--green);
  }
  .down {
    color: var(--red);
  }
  .muted {
    color: var(--muted);
  }
  .err {
    color: var(--red);
  }
</style>
