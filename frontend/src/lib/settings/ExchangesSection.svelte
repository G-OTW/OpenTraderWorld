<script>
  // Settings → Exchanges: each venue's regular hours in its own timezone, its lunch break,
  // whether it trades now, and its next holidays and half days. Reference data, read-only:
  // it ships with the app and is refreshed from the public repo once a week (or now, with
  // the button). The venue shown in the top bar is picked in Settings → Defaults.
  import Button from '$lib/ui/Button.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { settingsApi } from './api.js';
  import { t, locale } from '$lib/i18n';

  let data = $state(null);
  let error = $state('');
  let syncing = $state(false);
  let note = $state('');
  let search = $state('');
  let status = $state('all'); // 'all' | 'open' | 'closed'

  async function load() {
    try {
      data = await settingsApi.exchanges();
      error = '';
    } catch (e) {
      error = e.message;
    }
  }
  load();

  async function sync() {
    syncing = true;
    note = '';
    try {
      const r = await settingsApi.syncExchanges();
      note = $t(r.updated ? 'exchanges.updated' : 'exchanges.upToDate');
      await load();
    } catch (e) {
      error = e.message;
    } finally {
      syncing = false;
    }
  }

  const hm = (m) =>
    m == null ? '' : `${String(Math.floor(m / 60)).padStart(2, '0')}:${String(m % 60).padStart(2, '0')}`;
  const day = (iso) => (iso ? new Date(iso).toLocaleDateString() : '');
  const when = (iso) => (iso ? new Date(iso).toLocaleString() : '');
  const dateOnly = (d) => new Date(`${d}T12:00:00Z`).toLocaleDateString();

  // Upstream project the calendar is generated from, also listed in Settings → Credits.
  const PROJECT_URL = 'https://github.com/gerrymanoim/exchange_calendars';
  const SLOT = '\u0000';
  // The meta sentence split around {source}, so the source renders as a link.
  const meta = $derived(
    data
      ? $t('exchanges.meta', {
          source: SLOT,
          generated: day(data.generated),
          checked: data.checked_at ? when(data.checked_at) : $t('exchanges.never')
        }).split(SLOT)
      : []
  );

  // The calendar carries no country, so map each MIC to its ISO country code; the search
  // then matches the country name in English and in the UI language.
  const COUNTRY = {
    XASX: 'AU', XNZE: 'NZ', XTKS: 'JP', XKRX: 'KR', XSHG: 'CN', XHKG: 'HK', XTAI: 'TW',
    XSES: 'SG', XBOM: 'IN', XJSE: 'ZA', XETR: 'DE', XPAR: 'FR', XAMS: 'NL', XLON: 'GB',
    XSWX: 'CH', XMIL: 'IT', XMAD: 'ES', XSTO: 'SE', XNYS: 'US', XNAS: 'US', XCME: 'US',
    XTSE: 'CA', BVMF: 'BR', XMEX: 'MX'
  };
  function countryNames(mic, lang) {
    const cc = COUNTRY[mic];
    if (!cc) return '';
    const names = [];
    for (const l of ['en', lang]) {
      try {
        names.push(new Intl.DisplayNames([l], { type: 'region' }).of(cc));
      } catch {
        // unknown locale: the English name still matches
      }
    }
    return names.join(' ');
  }
  const norm = (s) =>
    s.normalize('NFD').replace(/\p{Diacritic}/gu, '').toLowerCase();
  const shown = $derived.by(() => {
    if (!data) return [];
    const q = norm(search.trim());
    return data.exchanges.filter(
      (x) =>
        (status === 'all' || x.status.open === (status === 'open')) &&
        (!q ||
          norm(`${x.name} ${x.code} ${x.mic} ${x.city} ${countryNames(x.mic, $locale)}`).includes(q))
    );
  });

  function special(u) {
    if (u.open == null && u.close == null) return $t('exchanges.holiday');
    if (u.close != null) return $t('exchanges.closesAt', { time: hm(u.close) });
    return $t('exchanges.opensAt', { time: hm(u.open) });
  }
</script>

<div class="section">
  <div class="head">
    <h2>{$t('exchanges.title')}</h2>
    <Button size="sm" icon="refresh-cw" loading={syncing} onclick={sync}>
      {$t('exchanges.sync')}
    </Button>
  </div>
  <p class="muted small">{$t('exchanges.subtitle')}</p>
  {#if data}
    <p class="callout small">
      <Icon name="info" size={15} />
      <span>
        {meta[0]}<a href={PROJECT_URL} target="_blank" rel="noopener noreferrer">{data.source}</a
        >{meta[1] ?? ''}
      </span>
    </p>
  {/if}
  {#if note}<p class="ok small">{note}</p>{/if}

  <ErrorText {error} />

  {#if data}
    <div class="filters">
      <input
        class="search"
        type="search"
        placeholder={$t('exchanges.search')}
        aria-label={$t('exchanges.search')}
        bind:value={search}
      />
      <div class="seg" role="group" aria-label={$t('exchanges.statusFilter')}>
        {#each ['all', 'open', 'closed'] as k (k)}
          <button
            type="button"
            class:on={status === k}
            aria-pressed={status === k}
            onclick={() => (status = k)}
          >
            {#if k !== 'all'}<span class="dot" class:open={k === 'open'}></span>{/if}
            {$t(`exchanges.${k}`)}
          </button>
        {/each}
      </div>
    </div>
    <table>
      <thead>
        <tr>
          <th></th>
          <th>{$t('exchanges.venue')}</th>
          <th>{$t('exchanges.hours')}</th>
          <th>{$t('exchanges.upcoming')}</th>
        </tr>
      </thead>
      <tbody>
        {#each shown as x (x.mic)}
          <tr class:home={x.mic === data.home}>
            <td class="code">
              <span
                class="dot"
                class:open={x.status.open}
                title={$t(x.status.open ? 'exchanges.open' : 'exchanges.closed')}
              ></span>
              {x.code}
            </td>
            <td>
              <div class="name">{x.name}</div>
              <div class="muted">{x.city} · {x.mic}</div>
            </td>
            <td>
              <div>
                {$t('exchanges.range', { open: hm(x.open), close: hm(x.close) })}
                {#if x.prior_day_open}<span class="muted">{$t('exchanges.priorDay')}</span>{/if}
              </div>
              {#if x.break_start != null}
                <div class="muted">
                  {$t('exchanges.break', { start: hm(x.break_start), end: hm(x.break_end) })}
                </div>
              {/if}
              <div class="muted">{x.timezone}</div>
            </td>
            <td>
              <div class="chips">
                {#each x.upcoming as u (u.date)}
                  <span class="chip" class:half={u.open != null || u.close != null}>
                    {dateOnly(u.date)} · {special(u)}
                  </span>
                {:else}
                  <span class="muted">{$t('exchanges.none')}</span>
                {/each}
              </div>
              {#if x.status.holidays_unknown}
                <div class="warn">{$t('exchanges.unknown', { date: dateOnly(x.covered_to) })}</div>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>

<style>
  .section {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-3);
  }
  h2 {
    font-size: var(--text-md);
    font-weight: var(--fw-medium);
  }
  .muted {
    color: var(--muted);
  }
  .small {
    font-size: var(--text-sm);
    line-height: 1.45;
    max-width: 78ch;
    margin: 0;
  }
  .callout {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    background: color-mix(in srgb, var(--accent) 8%, transparent);
    border: var(--hairline) solid color-mix(in srgb, var(--accent) 35%, var(--border));
    border-radius: var(--radius);
    color: var(--text);
  }
  .callout :global(.icon-svg) {
    flex: none;
    margin-top: 2px;
    color: var(--accent);
  }
  .callout a {
    color: var(--accent);
    font-weight: var(--fw-medium);
    text-decoration: underline;
  }
  .filters {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }
  .search {
    flex: 1;
    min-width: 0;
    max-width: 280px;
  }
  .seg {
    display: inline-flex;
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .seg button {
    display: inline-flex;
    align-items: center;
    padding: 5px 10px;
    font-size: var(--text-sm);
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
  }
  .seg button + button {
    border-left: var(--hairline) solid var(--border);
  }
  .seg button:hover {
    color: var(--text);
  }
  .seg button.on {
    background: var(--surface-2);
    color: var(--text);
  }
  .ok {
    color: var(--green);
  }
  .warn {
    color: var(--amber);
    font-size: 0.72rem;
    margin-top: 2px;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }
  th {
    text-align: left;
    font-weight: var(--fw-medium);
    color: var(--muted);
    padding: var(--space-1) var(--space-2);
    border-bottom: var(--hairline) solid var(--border);
  }
  td {
    padding: var(--space-2);
    border-bottom: var(--hairline) solid var(--border);
    vertical-align: top;
  }
  tr.home td {
    background: var(--surface-2);
  }
  .code {
    white-space: nowrap;
    font-weight: 600;
    letter-spacing: 0.04em;
  }
  .name {
    font-weight: var(--fw-medium);
  }
  .dot {
    display: inline-block;
    width: 7px;
    height: 7px;
    margin-right: 4px;
    border-radius: 50%;
    background: var(--muted);
    opacity: 0.6;
  }
  .dot.open {
    background: var(--green);
    opacity: 1;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .chip {
    font-size: 0.7rem;
    padding: 1px 6px;
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    color: var(--muted);
  }
  .chip.half {
    color: var(--amber);
  }
</style>
