<script>
  // `drift`: current allocation against the target, and the trades that close the gap.
  //
  // Display only. Nothing here places an order, which is why the suggestion is money and a
  // direction rather than a ticket.
  //
  // The deviation is drawn as diverging bars around 0 on the diverging palette: over and
  // under weight are opposite directions, not good and bad, so they are not green and red.
  import { t } from '$lib/i18n';
  import { fmtPct as fmtPlainPct, fmtSignedPct } from '$lib/format.js';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import Badge from '$lib/ui/Badge.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Bars from '$lib/ui/charts/Bars.svelte';
  import { fmtMoney, fmtNum, na } from '../api.js';
  import Unavailable from './Unavailable.svelte';
  import Sample from './Sample.svelte';
  import Panel from './Panel.svelte';

  let { block = null, currency = 'USD', onfix = null } = $props();
  const d = $derived(block?.data ?? null);
  const c = $derived(chartColors());
  const rows = $derived(d?.rows ?? []);
  const tone = (v) => (v > 0 ? c.diverge[2] : v < 0 ? c.diverge[0] : c.diverge[1]);
</script>

{#if block?.status !== 'ok' || !d}
  <Unavailable missing={block?.missing ?? []} sample={block?.sample} {onfix} />
{:else}
  <div class="pa-stack">
    <Panel title={$t('portfolios.analytics.drift.chartTitle')} subtitle={$t('portfolios.analytics.drift.chartSub')}>
      {#snippet actions()}
        <Badge tone={d.in_band ? 'success' : 'warn'} icon={d.in_band ? 'check' : 'alert-triangle'}>
          {d.in_band
            ? $t('portfolios.analytics.drift.inBand')
            : $t('portfolios.analytics.drift.outOfBand', { pct: fmtPlainPct(d.worst_deviation_pct) })}
        </Badge>
        <Button size="sm" variant="ghost" icon="pencil" onclick={() => onfix?.('targets')}>
          {$t('portfolios.analytics.drift.edit')}
        </Button>
      {/snippet}
      <Bars
        horizontal
        labels="ends"
        categories={rows.map((r) => ({ key: r.bucket, label: $t(`portfolios.class.${r.bucket}`) }))}
        series={[
          {
            key: 'dev',
            label: $t('portfolios.analytics.drift.deviation'),
            values: rows.map((r) => r.deviation_pct),
            colors: rows.map((r) => tone(r.deviation_pct))
          }
        ]}
        format={(v) => fmtSignedPct(v)}
        sensitive={false}
        legend="none"
      />
    </Panel>

    <Panel title={$t('portfolios.analytics.drift.tableTitle')} flush>
      <div class="pa-scroll">
        <table class="pa-table">
          <thead>
            <tr>
              <th>{$t('portfolios.analytics.drift.bucket')}</th>
              <th class="r">{$t('portfolios.analytics.drift.current')}</th>
              <th class="r">{$t('portfolios.analytics.drift.target')}</th>
              <th class="r">{$t('portfolios.analytics.drift.deviation')}</th>
              <th class="r">{$t('portfolios.analytics.drift.move')}</th>
            </tr>
          </thead>
          <tbody>
            {#each rows as r (r.bucket)}
              <tr>
                <td class="bk">
                  {$t(`portfolios.class.${r.bucket}`)}
                  {#if r.out_of_band}<Badge tone="warn">{$t('portfolios.analytics.drift.off')}</Badge>{/if}
                </td>
                <td class="r">{fmtPlainPct(r.current_pct)}</td>
                <td class="r pa-muted">{fmtPlainPct(r.target_pct)}</td>
                <td class="r">{fmtSignedPct(r.deviation_pct)}</td>
                <td class="r">{r.out_of_band ? fmtMoney(r.delta_value, currency) : ''}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </Panel>

    {#if d.unallocated.length}
      <!-- Held but not planned for. Reported, never folded into "other" or treated as a zero
           target: both hide a real hole in the plan. -->
      <p class="pa-warn unalloc">
        {$t('portfolios.analytics.drift.unallocated')}
        {#each d.unallocated as u (u.bucket)}
          <Badge tone="warn">{$t(`portfolios.class.${u.bucket}`)} {fmtPlainPct(u.current_pct)}</Badge>
        {/each}
      </p>
    {/if}

    {#if d.trades.length}
      <Panel title={$t('portfolios.analytics.drift.plan')} subtitle={$t('portfolios.analytics.drift.disclaimer')} flush>
        <table class="pa-table">
          <tbody>
            {#each d.trades as tr (tr.bucket)}
              <tr>
                <td class="side">
                  <Badge tone={tr.side === 'buy' ? 'success' : 'danger'}>{$t(`portfolios.kind.${tr.side}`)}</Badge>
                </td>
                <td>{$t(`portfolios.class.${tr.bucket}`)}</td>
                <td class="legs">
                  {#each tr.legs as leg (leg.asset_id)}
                    <span class="leg">{leg.symbol}{#if leg.units != null}<span class="pa-muted units">{fmtNum(leg.units, 4)}</span>{/if}</span>
                  {/each}
                </td>
                <td class="r">{fmtMoney(tr.amount, currency)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </Panel>
    {/if}

    <Sample sample={block.sample} />
  </div>
{/if}

<style>
  .unalloc {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }
  .side {
    width: 1%;
  }
  .legs {
    color: var(--text);
    white-space: normal;
  }
  .units {
    margin-left: var(--space-1);
    font-family: var(--mono);
  }
  .bk :global(.badge) {
    margin-left: var(--space-2);
  }
  .leg + .leg::before {
    content: ' · ';
    color: var(--dim);
  }
</style>
