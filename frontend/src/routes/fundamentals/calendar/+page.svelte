<script>
  // Fundamentals: Events. Upcoming earnings, IPOs and splits from the first connected
  // calendar provider, and recent central-bank moves from the BIS. Dated macro releases
  // stay in the Economic Calendar module.
  import Shell from '$lib/modules/fundamentals/Shell.svelte';
  import Section from '$lib/modules/fundamentals/Section.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import DataNote from '$lib/modules/fundamentals/DataNote.svelte';
  import { fundamentalsApi, fmtBig, fmtNum, fmtPct } from '$lib/modules/fundamentals/api.js';
  import { visibleSections } from '$lib/modules/fundamentals/prefs.svelte.js';
  import { t } from '$lib/i18n';

  const SECTIONS = ['earnings', 'ipos', 'corporate', 'banks'];
  let c = $state(null);
  let busy = $state(false);

  async function load(force = false) {
    busy = true;
    c = await fundamentalsApi.calendar(force);
    busy = false;
  }

  $effect(() => {
    load();
  });
  const sections = $derived(visibleSections('calendar', SECTIONS));
</script>

<Shell
  title={$t('fundamentals.cal.title')}
  subtitle={$t('fundamentals.cal.subtitle')}
  page="calendar"
  sections={SECTIONS.map((id) => ({ id, label: $t(`fundamentals.cal.${id}`) }))}
>
  {#if c}
    <DataNote notes={c.notes} {busy} onrefresh={() => load(true)} />
    {#each sections as id (id)}
      {#if id === 'earnings'}
        <Section key="calendar.earnings" title={$t('fundamentals.cal.earnings')} description={$t('fundamentals.cal.earningsDesc')}>
          <div class="fd-scroll">
            <table class="tbl">
              <thead>
                <tr>
                  <th>{$t('fundamentals.date')}</th>
                  <th>{$t('fundamentals.ticker')}</th>
                  <th>{$t('fundamentals.cal.time')}</th>
                  <th class="num">{$t('fundamentals.est.eps')}</th>
                  <th class="num">{$t('fundamentals.est.revenue')}</th>
                </tr>
              </thead>
              <tbody>
                {#each c.earnings as e, i (i)}
                  <tr>
                    <td>{e.date}</td>
                    <td><a href="/fundamentals/company?t={e.ticker}&tab=earnings">{e.ticker}</a></td>
                    <td>{#if e.time === 'BMO' || e.time === 'AMC'}<Badge tone={e.time === 'BMO' ? 'accent' : 'neutral'}>{e.time === 'BMO' ? $t('fundamentals.earn.bmo') : $t('fundamentals.earn.amc')}</Badge>{/if}</td>
                    <td class="num">{fmtNum(e.eps_estimate)}</td>
                    <td class="num">{fmtBig(e.revenue_estimate)}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        </Section>
      {:else if id === 'ipos'}
        <Section key="calendar.ipos" title={$t('fundamentals.cal.ipos')} description={$t('fundamentals.cal.iposDesc')}>
          <div class="fd-scroll">
            <table class="tbl">
              <thead>
                <tr>
                  <th>{$t('fundamentals.date')}</th>
                  <th>{$t('fundamentals.name')}</th>
                  <th>{$t('fundamentals.company.exchange')}</th>
                  <th class="num">{$t('fundamentals.cal.priceRange')}</th>
                  <th class="num">{$t('fundamentals.cal.size')}</th>
                </tr>
              </thead>
              <tbody>
                {#each c.ipos as i, n (n)}
                  <tr>
                    <td>{i.date}</td>
                    <td><strong>{i.ticker}</strong> <span class="fd-muted">{i.company}</span></td>
                    <td>{i.exchange}</td>
                    <td class="num">{i.range}</td>
                    <td class="num">{fmtBig(i.size)}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        </Section>
      {:else if id === 'corporate'}
        <Section key="calendar.corporate" title={$t('fundamentals.cal.corporate')} description={$t('fundamentals.cal.corporateDesc')}>
          <table class="tbl">
            <thead><tr><th>{$t('fundamentals.date')}</th><th>{$t('fundamentals.ticker')}</th><th>{$t('fundamentals.type')}</th><th>{$t('fundamentals.alt.description')}</th></tr></thead>
            <tbody>
              {#each c.corporate as a, i (i)}
                <tr><td>{a.date}</td><td>{a.ticker}</td><td>{a.type}</td><td class="fd-muted">{a.detail}</td></tr>
              {/each}
            </tbody>
          </table>
        </Section>
      {:else}
        <Section key="calendar.banks" title={$t('fundamentals.cal.banks')} description={$t('fundamentals.cal.banksDesc')}>
          <table class="tbl">
            <thead><tr><th>{$t('fundamentals.date')}</th><th>{$t('fundamentals.macro.bank')}</th><th class="num">{$t('fundamentals.macro.lastChange')}</th><th class="num">{$t('fundamentals.macro.rate')}</th></tr></thead>
            <tbody>
              {#each c.central_banks as b (b.bank)}
                <tr>
                  <td>{b.date}</td>
                  <td>{b.bank}</td>
                  <td class="num" class:fd-up={b.change > 0} class:fd-down={b.change < 0}>{b.change > 0 ? '+' : ''}{Math.round((b.change ?? 0) * 100)} bp</td>
                  <td class="num">{fmtPct(b.current, 2)}</td>
                </tr>
              {:else}
                <tr><td colspan="4" class="fd-muted">{$t('fundamentals.none')}</td></tr>
              {/each}
            </tbody>
          </table>
        </Section>
      {/if}
    {/each}
  {/if}
</Shell>
