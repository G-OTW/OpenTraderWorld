<script>
  // Who owns it: institutional (13F) holders, insider transactions (Form 4, EDGAR),
  // short interest and days to cover. The borrow fee needs IBKR (not wired yet).
  import Section from '../Section.svelte';
  import Chart from '../Chart.svelte';
  import StatCard from '$lib/ui/StatCard.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import DataNote from '../DataNote.svelte';
  import { fundamentalsApi, fmtBig, fmtNum, fmtPct } from '../api.js';
  import { t } from '$lib/i18n';

  let { company } = $props();
  let o = $state(null);
  let notes = $state([]);
  let busy = $state(false);

  async function load(force = false) {
    busy = true;
    const r = await fundamentalsApi.tab(company.ticker, 'ownership', force);
    o = r.data;
    notes = r.notes;
    busy = false;
  }

  $effect(() => {
    load();
  });
  let insiders = $state([]);

  $effect(() => {
    fundamentalsApi.insiders(company.ticker).then((x) => (insiders = x)).catch(() => (insiders = []));
  });

  const tone = (type) => (type === 'Buy' ? 'success' : type === 'Sell' ? 'danger' : 'neutral');
</script>

<DataNote {notes} {busy} onrefresh={() => load(true)} />

{#if o}  <div class="fd-stats">
    <StatCard label={$t('fundamentals.own.institutions')} value={fmtPct(o.breakdown.institutions)} hint={o.holders_quarter ?? ''} />
    <StatCard label={$t('fundamentals.own.shortShares')} value={fmtBig(o.short_shares)} hint={o.short_date ?? ''} />
    <StatCard label={$t('fundamentals.own.shortFloat')} value={fmtPct(o.short_interest.at(-1)?.[1], 2)} />
    <StatCard label={$t('fundamentals.own.borrowFee')} value={fmtPct(o.borrow_fee, 2)} />
    <StatCard label={$t('fundamentals.own.daysToCover')} value={fmtNum(o.days_to_cover, 1)} />
  </div>

  <div class="fd-grid">
    <Section key="company.holders" title={$t('fundamentals.own.holders')} description={$t('fundamentals.own.holdersDesc')} source="13F">
      <div class="fd-scroll">
        <table class="tbl">
          <thead>
            <tr>
              <th>{$t('fundamentals.own.holder')}</th>
              <th class="num">%</th>
              <th class="num">{$t('fundamentals.own.shares')}</th>
              <th class="num">{$t('fundamentals.own.change')}</th>
            </tr>
          </thead>
          <tbody>
            {#each o.holders as h, i (i)}
              <tr>
                <td>{h.holder}</td>
                <td class="num">{fmtNum(h.pct)}</td>
                <td class="num">{fmtBig(h.shares)}</td>
                <td class="num" class:fd-up={h.change_pct > 0} class:fd-down={h.change_pct < 0}>{fmtPct(h.change_pct, 2, true)}</td>
              </tr>
            {:else}
              <tr><td colspan="4" class="fd-muted">{$t('fundamentals.none')}</td></tr>
            {/each}
          </tbody>
        </table>
      </div>
    </Section>
    <Section key="company.short" title={$t('fundamentals.own.shortInterest')} description={$t('fundamentals.own.shortDesc')} source="FINRA">
      <Chart series={[{ name: $t('fundamentals.own.shortFloat'), data: o.short_interest, area: true }]} legend={false} yFormat={(v) => `${Number(v).toFixed(2)}%`} height={260} />
    </Section>
  </div>

  <Section key="company.insiders" title={$t('fundamentals.own.insiderTx')} description={$t('fundamentals.own.insiderDesc')} source="Form 4">
    <div class="fd-scroll">
      <table class="tbl">
        <thead>
          <tr>
            <th>{$t('fundamentals.date')}</th>
            <th>{$t('fundamentals.own.insider')}</th>
            <th>{$t('fundamentals.own.role')}</th>
            <th>{$t('fundamentals.type')}</th>
            <th class="num">{$t('fundamentals.own.shares')}</th>
            <th class="num">{$t('fundamentals.price')}</th>
            <th class="num">{$t('fundamentals.value')}</th>
          </tr>
        </thead>
        <tbody>
          {#each insiders as x, i (i)}
            <tr>
              <td>{x.date}</td>
              <td>{x.insider}</td>
              <td>{x.role}</td>
              <td><Badge tone={tone(x.type)}>{x.type}</Badge></td>
              <td class="num">{x.acquired ? '+' : '−'}{x.shares.toLocaleString()}</td>
              <td class="num">{fmtNum(x.price)}</td>
              <td class="num">{fmtBig(x.value)}</td>
            </tr>
          {:else}
            <tr><td colspan="7" class="fd-muted">{$t('fundamentals.own.noInsiders')}</td></tr>
          {/each}
        </tbody>
      </table>
    </div>
  </Section>
{/if}
