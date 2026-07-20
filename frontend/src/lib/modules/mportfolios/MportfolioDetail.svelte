<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Holdings detail for one manager's portfolio, shown in a wide modal. Loads on open.
  import Modal from '$lib/ui/Modal.svelte';
  import { mportfoliosApi, fmtValue, fmtNum, fmtPrice, fmtPct } from './api.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  // `onsnapshot(slug)` is optional; when provided a 📷 button is shown that lets the user save a
  // frozen copy of this live portfolio. The parent owns the API call and any feedback/reload.
  let { slug = $bindable(null), onsnapshot } = $props();

  let portfolio = $state(null);
  let holdings = $state([]);
  let loading = $state(false);
  let error = $state('');
  let snapping = $state(false);

  let open = $derived(slug != null);

  async function snapshot() {
    if (!portfolio || snapping || !onsnapshot) return;
    snapping = true;
    try {
      await onsnapshot(portfolio.slug ?? slug);
    } finally {
      snapping = false;
    }
  }

  $effect(() => {
    if (!slug) return;
    loading = true;
    error = '';
    portfolio = null;
    holdings = [];
    mportfoliosApi
      .detail(slug)
      .then((r) => {
        portfolio = r.portfolio;
        holdings = r.holdings;
      })
      .catch((e) => (error = e.message))
      .finally(() => (loading = false));
  });

  function close() {
    slug = null;
  }
</script>

<Modal {open} size="lg" title={portfolio?.name ?? $t('mportfolios.detail.title')} onclose={close}>
  {#if loading}
    <p class="muted">{$t('common.loading')}</p>
  {:else if error}
    <ErrorText {error} copyable />
  {:else if portfolio}
    <div class="meta">
      {#if portfolio.period}<span class="chip">{portfolio.period}</span>{/if}
      <span class="chip">{fmtValue(portfolio.value_num, portfolio.value_text)}</span>
      <span class="chip">{$t('mportfolios.detail.holdingsCount', { count: portfolio.stock_count })}</span>
      {#if onsnapshot}
        <button class="snap" onclick={snapshot} disabled={snapping} title={$t('mportfolios.detail.saveSnapshotTitle')}>
          {#if snapping}…{:else}<Icon name="camera" size={13} />{/if} {$t('mportfolios.detail.snapshot')}
        </button>
      {/if}
      <a class="src" href={portfolio.source_url} target="_blank" rel="noopener">{$t('mportfolios.detail.viewOn')} Dataroma <Icon name="external-link" size={11} /></a>
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
  .snap {
    margin-left: auto;
    background: transparent;
    border: 0.5px solid var(--border-control);
    border-radius: 0;
    padding: 2px var(--space-2);
    color: var(--text);
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .snap:hover:not(:disabled) {
    background: var(--surface-2);
  }
  .snap:disabled {
    opacity: 0.6;
    cursor: default;
  }
  /* When the snapshot button is present it takes the right slot; otherwise the source link does. */
  .snap ~ .src {
    margin-left: var(--space-2);
  }
  .src {
    margin-left: auto;
    color: var(--muted);
    font-size: var(--text-sm);
    text-decoration: none;
  }
  .table-wrap {
    overflow-x: auto;
  }
  th,
  td {
    padding: var(--space-1) var(--space-2);
    text-align: right;
    border-bottom: 0.5px solid var(--border);
  }
  /* Numeric columns: right-aligned figures in the tabular mono. */
  td:not(.l) {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  th {
    color: var(--dim);
    font-weight: var(--fw-medium);
    position: sticky;
    top: 0;
    background: var(--surface);
  }
  .l {
    text-align: left;
  }
  .tk {
    font-family: var(--mono);
    font-weight: var(--fw-medium);
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
</style>
