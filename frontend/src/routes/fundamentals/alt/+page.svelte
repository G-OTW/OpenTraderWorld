<script>
  // Fundamentals: Alternative data. Congress trades (the latest across all members, or one
  // company's), and per company: lobbying spend, federal contracts and patents granted.
  // The company is the one Company has open; the public sources match it on the name EDGAR
  // stores, so a ticker typed here is opened (stored) first. Tabs follow Customize.
  import Shell from '$lib/modules/fundamentals/Shell.svelte';
  import Section from '$lib/modules/fundamentals/Section.svelte';
  import Chart from '$lib/modules/fundamentals/Chart.svelte';
  import DataNote from '$lib/modules/fundamentals/DataNote.svelte';
  import SymbolPicker from '$lib/modules/fundamentals/SymbolPicker.svelte';
  import Tabs from '$lib/ui/Tabs.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { fundamentalsApi, fmtBig } from '$lib/modules/fundamentals/api.js';
  import { prefs, visibleSections } from '$lib/modules/fundamentals/prefs.svelte.js';
  import { t } from '$lib/i18n';
  import { untrack } from 'svelte';

  const TABS = ['congress', 'lobbying', 'contracts', 'patents'];
  const tabs = $derived(visibleSections('alt', TABS));
  let tab = $state('congress');
  // Congress trades: the latest across all members, or the open company's.
  let scope = $state('_');
  let stored = $state([]);
  let opening = $state(false);
  let openError = $state('');
  let data = $state(null);
  let notes = $state([]);
  let busy = $state(false);

  const ticker = $derived(prefs.company.ticker ?? '');
  const subject = $derived(tab === 'congress' ? scope : ticker);

  $effect(() => {
    fundamentalsApi.companies().then((c) => (stored = c)).catch(() => {});
  });
  $effect(() => {
    if (!tabs.includes(tab)) tab = tabs[0] ?? 'congress';
  });
  $effect(() => {
    if (scope !== '_' && scope !== ticker) scope = ticker || '_';
  });

  let seq = 0;
  async function load(force = false) {
    const n = ++seq;
    data = null;
    notes = [];
    if (!subject) return;
    busy = true;
    // The public sources need the company's registered name: open it first if needed.
    if (subject !== '_' && !untrack(() => stored).some((c) => c.ticker === subject)) {
      try {
        await fundamentalsApi.company(subject);
      } catch {
        /* the dataset's own note names the fix */
      }
    }
    const r = await fundamentalsApi.alt(tab, subject, force);
    if (n !== seq) return;
    data = r.data;
    notes = r.notes;
    busy = false;
  }

  $effect(() => {
    tab;
    subject;
    load();
  });

  const loadStored = async () => (stored = await fundamentalsApi.companies());

  // A stored company moves to the front of the recent ones; another is opened (resolved,
  // stored, fetched) first.
  async function open(tk) {
    opening = true;
    openError = '';
    try {
      if (stored.some((c) => c.ticker === tk)) await fundamentalsApi.openedCompany(tk);
      else await fundamentalsApi.company(tk);
      prefs.company.ticker = tk;
      await loadStored();
    } catch (err) {
      openError = err.message;
    }
    opening = false;
  }

  async function follow(tk, followed) {
    await fundamentalsApi.followCompany(tk, followed);
    await loadStored();
  }

  const lookup = async (q) =>
    (await fundamentalsApi.searchCompanies(q)).map((s) => ({
      ticker: s.ticker,
      name: s.name,
      detail: [s.exchange, s.sector].filter(Boolean).join(' · ')
    }));

  const tone = (type) => (type === 'purchase' ? 'success' : type === 'sale' ? 'danger' : 'neutral');
  const matched = (m) => (Array.isArray(m) ? m.join(', ') : m);
</script>

<Shell
  title={$t('fundamentals.alt.title')}
  subtitle={$t('fundamentals.alt.subtitle')}
  page="alt"
  sections={TABS.map((id) => ({ id, label: $t(`fundamentals.alt.${id}`) }))}
>
  <div class="fd-toolbar">
    <SymbolPicker items={stored} value={ticker} placeholder={$t('fundamentals.company.search')} {lookup} onpick={open} onfollow={follow} busy={opening} />
  </div>
  {#if openError}<ErrorText>{openError}</ErrorText>{/if}

  <Tabs tabs={tabs.map((id) => ({ id, label: $t(`fundamentals.alt.${id}`) }))} bind:value={tab} ariaLabel={$t('fundamentals.alt.title')} />

  {#if tab === 'congress'}
    <div class="fd-row scope">
      <button class="chip" class:active={scope === '_'} onclick={() => (scope = '_')}>{$t('fundamentals.alt.latest')}</button>
      {#if ticker}
        <button class="chip" class:active={scope === ticker} onclick={() => (scope = ticker)}>{ticker}</button>
      {/if}
    </div>
  {/if}

  {#if !subject}
    <p class="fd-note">{$t('fundamentals.alt.pick')}</p>
  {:else}
    <DataNote {notes} {busy} onrefresh={() => load(true)} />

    {#if data && tab === 'congress'}
      <Section key="alt.congress" title={$t('fundamentals.alt.congress')} description={$t('fundamentals.alt.congressDesc')}>
        <div class="fd-scroll">
          <table class="tbl">
            <thead>
              <tr>
                <th>{$t('fundamentals.alt.member')}</th>
                <th>{$t('fundamentals.alt.chamber')}</th>
                <th>{$t('fundamentals.ticker')}</th>
                <th>{$t('fundamentals.type')}</th>
                <th class="num">{$t('fundamentals.alt.amount')}</th>
                <th>{$t('fundamentals.alt.traded')}</th>
                <th>{$t('fundamentals.alt.disclosed')}</th>
              </tr>
            </thead>
            <tbody>
              {#each data.trades as c, i (i)}
                <tr>
                  <td>{c.member}{#if c.party} <span class="fd-muted">({c.party})</span>{/if}</td>
                  <td>{$t(`fundamentals.alt.chamber.${c.chamber}`)}</td>
                  <td>{#if c.ticker}<a href="/fundamentals/company?t={c.ticker}">{c.ticker}</a>{/if}</td>
                  <td><Badge tone={tone(c.type)}>{$t(`fundamentals.alt.trade.${c.type}`)}</Badge></td>
                  <td class="num">{c.amount}</td>
                  <td>{c.traded}</td>
                  <td class="fd-muted">{c.disclosed}</td>
                </tr>
              {:else}
                <tr><td colspan="7" class="fd-muted">{$t('fundamentals.none')}</td></tr>
              {/each}
            </tbody>
          </table>
        </div>
      </Section>
    {:else if data && tab === 'lobbying'}
      <div class="fd-grid">
        <Section key="alt.lobbyingSpend" title={$t('fundamentals.alt.lobbyingSpend')} description={$t('fundamentals.alt.matched', { name: matched(data.match) })}>
          <Chart series={[{ name: $t('fundamentals.alt.lobbying'), data: data.series, area: true }]} legend={false} height={220} />
        </Section>
        <Section key="alt.lobbying" title={$t('fundamentals.alt.lobbying')} description={$t('fundamentals.alt.lobbyingDesc')}>
          <div class="fd-scroll">
            <table class="tbl">
              <thead>
                <tr>
                  <th>{$t('fundamentals.period')}</th>
                  <th>{$t('fundamentals.alt.registrant')}</th>
                  <th class="num">{$t('fundamentals.alt.amount')}</th>
                  <th>{$t('fundamentals.alt.issues')}</th>
                </tr>
              </thead>
              <tbody>
                {#each data.filings as l, i (i)}
                  <tr>
                    <td>{#if l.url}<a href={l.url} target="_blank" rel="noopener noreferrer">{l.period}</a>{:else}{l.period}{/if}</td>
                    <td>{l.registrant}</td>
                    <td class="num">{fmtBig(l.amount)}</td>
                    <td class="fd-muted">{l.issues}</td>
                  </tr>
                {:else}
                  <tr><td colspan="4" class="fd-muted">{$t('fundamentals.none')}</td></tr>
                {/each}
              </tbody>
            </table>
          </div>
        </Section>
      </div>
    {:else if data && tab === 'contracts'}
      <div class="fd-grid">
        <Section key="alt.contractsSpend" title={$t('fundamentals.alt.obligations')} description={$t('fundamentals.alt.matched', { name: matched(data.match) })}>
          <Chart series={[{ name: $t('fundamentals.alt.obligations'), data: data.series, area: true }]} legend={false} height={220} />
        </Section>
        <Section key="alt.contracts" title={$t('fundamentals.alt.contracts')} description={$t('fundamentals.alt.contractsDesc')}>
          <div class="fd-scroll">
            <table class="tbl">
              <thead>
                <tr>
                  <th>{$t('fundamentals.date')}</th>
                  <th>{$t('fundamentals.alt.agency')}</th>
                  <th class="num">{$t('fundamentals.alt.amount')}</th>
                  <th>{$t('fundamentals.alt.description')}</th>
                </tr>
              </thead>
              <tbody>
                {#each data.awards as g, i (i)}
                  <tr>
                    <td>{#if g.url}<a href={g.url} target="_blank" rel="noopener noreferrer">{g.date}</a>{:else}{g.date}{/if}</td>
                    <td>{g.agency}</td>
                    <td class="num">{fmtBig(g.amount)}</td>
                    <td class="fd-muted">{g.description}</td>
                  </tr>
                {:else}
                  <tr><td colspan="4" class="fd-muted">{$t('fundamentals.none')}</td></tr>
                {/each}
              </tbody>
            </table>
          </div>
        </Section>
      </div>
    {:else if data && tab === 'patents'}
      <Section
        key="alt.patents"
        title={$t('fundamentals.alt.patentsTotal', { n: data.total })}
        description={`${$t('fundamentals.alt.patentsDesc')} ${$t('fundamentals.alt.matched', { name: matched(data.match) })}`}
      >
        <Chart series={[{ name: $t('fundamentals.alt.patents'), data: data.series, area: true }]} legend={false} height={260} />
      </Section>
    {/if}
  {/if}
</Shell>

<style>
  .scope {
    margin: var(--space-3) 0;
  }
</style>
