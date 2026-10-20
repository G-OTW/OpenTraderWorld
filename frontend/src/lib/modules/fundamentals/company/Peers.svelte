<script>
  // Side-by-side valuation and profitability against peers. Ratios show for peers that
  // are stored here too (open one to fill its row). Click a ticker to open it.
  import Section from '../Section.svelte';
  import DataNote from '../DataNote.svelte';
  import { fundamentalsApi, fmtBig, fmtNum, fmtPct } from '../api.js';
  import { t } from '$lib/i18n';

  let { company, onpick } = $props();
  let v = $state(null);
  let notes = $state([]);
  let busy = $state(false);

  async function load(force = false) {
    busy = true;
    const r = await fundamentalsApi.tab(company.ticker, 'peers', force);
    v = r.data;
    notes = r.notes;
    busy = false;
  }

  $effect(() => {
    load();
  });
  const rows = $derived(v?.peers ?? []);
</script>

<DataNote {notes} {busy} onrefresh={() => load(true)} />

<Section key="company.peers" title={$t('fundamentals.tab.peers')} description={$t('fundamentals.peers.desc')}>
  <div class="fd-scroll">
    <table class="tbl">
      <thead>
        <tr>
          <th>{$t('fundamentals.ticker')}</th>
          <th>{$t('fundamentals.name')}</th>
          <th class="num">{$t('fundamentals.metric.marketCap')}</th>
          <th class="num">{$t('fundamentals.metric.pe')}</th>
          <th class="num">{$t('fundamentals.metric.evEbitda')}</th>
          <th class="num">{$t('fundamentals.metric.grossMargin')}</th>
          <th class="num">{$t('fundamentals.metric.netMargin')}</th>
          <th class="num">{$t('fundamentals.metric.roe')}</th>
          <th class="num">{$t('fundamentals.metric.divYield')}</th>
        </tr>
      </thead>
      <tbody>
        {#each rows as p (p.ticker)}
          <tr class:self={p.self}>
            <td><button class="link" onclick={() => onpick?.(p.ticker)}>{p.ticker}</button></td>
            <td>{p.name}</td>
            <td class="num">{fmtBig(p.market_cap)}</td>
            <td class="num">{fmtNum(p.pe, 1)}</td>
            <td class="num">{fmtNum(p.ev_ebitda, 1)}</td>
            <td class="num">{fmtPct(p.gross_margin)}</td>
            <td class="num">{fmtPct(p.net_margin)}</td>
            <td class="num">{fmtPct(p.roe)}</td>
            <td class="num">{fmtPct(p.dividend_yield, 2)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
</Section>

<style>
  tr.self td {
    background: color-mix(in srgb, var(--accent) 8%, transparent);
    font-weight: var(--fw-medium);
  }
</style>
