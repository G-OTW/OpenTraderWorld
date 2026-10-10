<script>
  // Analyst consensus, revisions, ratings split, price target, rating actions and guidance.
  import Section from '../Section.svelte';
  import Chart from '../Chart.svelte';
  import DataNote from '../DataNote.svelte';
  import { fundamentalsApi, fmtBig, fmtNum } from '../api.js';
  import { t } from '$lib/i18n';

  let { company } = $props();
  let e = $state(null);
  let notes = $state([]);
  let busy = $state(false);

  async function load(force = false) {
    busy = true;
    const r = await fundamentalsApi.tab(company.ticker, 'estimates', force);
    e = r.data;
    notes = r.notes;
    busy = false;
  }

  $effect(() => {
    load();
  });

  const RATING_KEYS = ['strong_buy', 'buy', 'hold', 'sell', 'strong_sell'];
  // The current price is ours (the stored close), not the provider's.
  const pt = $derived(e?.price_target ? { ...e.price_target, current: company.price } : null);
  const ranged = $derived(pt && pt.low != null && pt.high != null && pt.high > pt.low);
  const pos = (v) => (ranged && v != null ? Math.min(100, Math.max(0, ((v - pt.low) / (pt.high - pt.low)) * 100)) : 0);
</script>

<DataNote {notes} {busy} onrefresh={() => load(true)} />

{#if e}
  <Section key="company.consensus" title={$t('fundamentals.est.consensus')} description={$t('fundamentals.est.consensusDesc')}>
    <div class="fd-scroll">
      <table class="tbl">
        <thead>
          <tr>
            <th>{$t('fundamentals.period')}</th>
            <th class="num">{$t('fundamentals.est.eps')}</th>
            <th class="num">{$t('fundamentals.est.low')}</th>
            <th class="num">{$t('fundamentals.est.high')}</th>
            <th class="num">{$t('fundamentals.est.revenue')}</th>
            <th class="num">{$t('fundamentals.est.analysts')}</th>
            <th class="num">{$t('fundamentals.est.revisions')}</th>
          </tr>
        </thead>
        <tbody>
          {#each e.consensus as c (c.period)}
            <tr>
              <td>{c.period}</td>
              <td class="num">{fmtNum(c.eps_mean)}</td>
              <td class="num">{fmtNum(c.eps_low)}</td>
              <td class="num">{fmtNum(c.eps_high)}</td>
              <td class="num">{fmtBig(c.revenue_mean)}</td>
              <td class="num">{c.analysts ?? '·'}</td>
              <td class="num">
                {#if c.revisions_up_30d != null || c.revisions_down_30d != null}
                  <span class="fd-up">▲ {c.revisions_up_30d ?? 0}</span>&nbsp;&nbsp;<span class="fd-down">▼ {c.revisions_down_30d ?? 0}</span>
                {:else}·{/if}
              </td>
            </tr>
          {:else}
            <tr><td colspan="7" class="fd-muted">{$t('fundamentals.none')}</td></tr>
          {/each}
        </tbody>
      </table>
    </div>
  </Section>

  <div class="fd-grid">
    <Section key="company.ratings" title={$t('fundamentals.est.ratings')} description={$t('fundamentals.est.ratingsDesc')}>
      {#if e.ratings}
        <Chart
          categories={RATING_KEYS.map((k) => $t(`fundamentals.est.r.${k}`))}
          series={[{ name: $t('fundamentals.est.analysts'), type: 'bar', data: RATING_KEYS.map((k) => e.ratings[k]) }]}
          legend={false}
          height={220}
        />
      {:else}
        <p class="fd-muted">{$t('fundamentals.none')}</p>
      {/if}
    </Section>
    <Section key="company.target" title={$t('fundamentals.est.target')} description={$t('fundamentals.est.targetDesc')}>
      {#if !pt}
        <p class="fd-muted">{$t('fundamentals.none')}</p>
      {:else}
      {#if ranged}
      <div class="pt">
        <div class="track">
          <span class="mark cur" style="left: {pos(pt.current)}%" title={$t('fundamentals.est.current')}></span>
          <span class="mark mean" style="left: {pos(pt.mean)}%" title={$t('fundamentals.est.mean')}></span>
        </div>
        <div class="fd-row ends"><span>{$t('fundamentals.est.low')} {fmtNum(pt.low)}</span><span>{$t('fundamentals.est.high')} {fmtNum(pt.high)}</span></div>
        <div class="fd-row legend fd-muted"><span><i class="dot cur"></i>{$t('fundamentals.est.current')}</span><span><i class="dot mean"></i>{$t('fundamentals.est.mean')}</span></div>
      </div>
      {/if}
      <dl class="fd-kv">
        <dt>{$t('fundamentals.est.current')}</dt><dd>{fmtNum(pt.current)}</dd>
        <dt>{$t('fundamentals.est.mean')}</dt><dd>{fmtNum(pt.mean)}</dd>
        <dt>{$t('fundamentals.est.median')}</dt><dd>{fmtNum(pt.median)}</dd>
        <dt>{$t('fundamentals.est.upside')}</dt><dd>{pt.mean != null && pt.current ? `${fmtNum(((pt.mean - pt.current) / pt.current) * 100, 1)}%` : '·'}</dd>
      </dl>
      {/if}
    </Section>
  </div>

  <div class="fd-grid">
    <Section key="company.actions" title={$t('fundamentals.est.actions')} description={$t('fundamentals.est.actionsDesc')}>
      <table class="tbl">
        <thead>
          <tr>
            <th>{$t('fundamentals.date')}</th>
            <th>{$t('fundamentals.est.firm')}</th>
            <th>{$t('fundamentals.est.action')}</th>
            <th>{$t('fundamentals.est.rating')}</th>
            <th class="num">{$t('fundamentals.est.targetShort')}</th>
          </tr>
        </thead>
        <tbody>
          {#each e.actions as a, i (i)}
            <tr><td>{a.date}</td><td>{a.firm}</td><td>{a.action}</td><td>{a.rating}</td><td class="num">{fmtNum(a.target)}</td></tr>
          {:else}
            <tr><td colspan="5" class="fd-muted">{$t('fundamentals.none')}</td></tr>
          {/each}
        </tbody>
      </table>
    </Section>
    <Section key="company.guidance" title={$t('fundamentals.est.guidance')} description={$t('fundamentals.est.guidanceDesc')}>
      <table class="tbl">
        <thead>
          <tr>
            <th>{$t('fundamentals.period')}</th>
            <th>{$t('fundamentals.est.metric')}</th>
            <th class="num">{$t('fundamentals.est.rangeCol')}</th>
            <th>{$t('fundamentals.est.issued')}</th>
          </tr>
        </thead>
        <tbody>
          {#each e.guidance ?? [] as g, i (i)}
            <tr>
              <td>{g.period}</td>
              <td>{g.metric}</td>
              <td class="num">{g.metric.includes('%') ? `${fmtNum(g.low, 1)} to ${fmtNum(g.high, 1)}` : `${fmtBig(g.low)} to ${fmtBig(g.high)}`}</td>
              <td class="fd-muted">{g.issued}</td>
            </tr>
          {:else}
            <tr><td colspan="4" class="fd-muted">{$t('fundamentals.est.noGuidance')}</td></tr>
          {/each}
        </tbody>
      </table>
    </Section>
  </div>
{/if}

<style>
  .pt {
    margin: var(--space-2) 0 var(--space-6);
  }
  .track {
    position: relative;
    height: 6px;
    border-radius: 3px;
    background: linear-gradient(90deg, var(--red), var(--amber), var(--green));
    opacity: 0.8;
  }
  .mark {
    position: absolute;
    top: -5px;
    width: 3px;
    height: 16px;
    border-radius: 2px;
    transform: translateX(-50%);
  }
  .cur {
    background: var(--text);
  }
  .mean {
    background: var(--accent);
  }
  .ends {
    justify-content: space-between;
    margin-top: var(--space-3);
    font-family: var(--mono);
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .legend {
    gap: var(--space-4);
    margin-top: var(--space-2);
  }
  .dot {
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 2px;
    margin-right: var(--space-1);
  }
</style>
