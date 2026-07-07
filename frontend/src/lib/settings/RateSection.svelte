<script>
  // API rate dashboard. Shows per-provider outbound-call volume for a window (today by
  // default) alongside each provider's published limit, plus a list of the most recent
  // over-limit responses. Observe-only: the backend never throttles — this just surfaces
  // usage and flags rate-limit answers so you can spot when a free tier is exhausted.
  import { onMount } from 'svelte';
  import { settingsApi } from '$lib/settings/api.js';
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';

  let days = $state(1);
  let providers = $state([]);
  let events = $state([]);
  let loading = $state(true);
  let error = $state('');

  onMount(reload);

  async function reload() {
    loading = true;
    error = '';
    try {
      const r = await settingsApi.rateUsage(days);
      providers = r.providers ?? [];
      events = r.events ?? [];
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  function setWindow(d) {
    days = d;
    reload();
  }

  const fmtTime = (iso) => {
    if (!iso) return '—';
    try {
      return new Date(iso).toLocaleString();
    } catch {
      return iso;
    }
  };
</script>

<div class="section">
  <div class="head">
    <h2>{$t('settings.rate.title')}</h2>
    <div class="toolbar">
      <label class="win">
        <span>{$t('settings.rate.window')}</span>
        <select value={String(days)} onchange={(e) => setWindow(Number(e.currentTarget.value))}>
          <option value="1">{$t('settings.rate.today')}</option>
          <option value="7">{$t('settings.rate.days', { count: 7 })}</option>
          <option value="30">{$t('settings.rate.days', { count: 30 })}</option>
        </select>
      </label>
      <button class="ghost" onclick={reload}>
        <Icon name="refresh-cw" size={13} /> {$t('common.refresh')}
      </button>
    </div>
  </div>
  <p class="muted small">{$t('settings.rate.subtitle')}</p>

  {#if error}<p class="err">{error}</p>{/if}

  {#if loading}
    <p class="muted">{$t('common.loading')}</p>
  {:else if !providers.length}
    <p class="muted">{$t('settings.rate.noData')}</p>
  {:else}
    <div class="tablewrap">
      <table>
        <thead>
          <tr>
            <th>{$t('settings.rate.provider')}</th>
            <th class="num">{$t('settings.rate.requests')}</th>
            <th class="num">{$t('settings.rate.limited')}</th>
            <th class="num">{$t('settings.rate.errors')}</th>
            <th>{$t('settings.rate.lastAt')}</th>
            <th>{$t('settings.rate.limit')}</th>
          </tr>
        </thead>
        <tbody>
          {#each providers as p (p.provider)}
            <tr class:hot={p.limited > 0}>
              <td class="prov">
                {p.provider}
                {#if p.host}<span class="host">{p.host}</span>{/if}
              </td>
              <td class="num strong">{p.requests}</td>
              <td class="num">
                {#if p.limited > 0}<span class="badge limited">{p.limited}</span>{:else}0{/if}
              </td>
              <td class="num">
                {#if p.errors > 0}<span class="badge err-badge">{p.errors}</span>{:else}0{/if}
              </td>
              <td class="time">{fmtTime(p.last_at)}</td>
              <td class="limit">{p.limit ?? ''}{#if !p.limit}<span class="muted">{$t('settings.rate.noLimit')}</span>{/if}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}

  <h3>{$t('settings.rate.eventsTitle')}</h3>
  {#if loading}
    <p class="muted">{$t('common.loading')}</p>
  {:else if !events.length}
    <p class="muted small">{$t('settings.rate.noEvents')}</p>
  {:else}
    <div class="tablewrap events">
      <table>
        <thead>
          <tr>
            <th>{$t('settings.rate.lastAt')}</th>
            <th>{$t('settings.rate.provider')}</th>
            <th>{$t('settings.rate.status')}</th>
            <th>{$t('settings.rate.detail')}</th>
          </tr>
        </thead>
        <tbody>
          {#each events as ev (ev.id)}
            <tr>
              <td class="time">{fmtTime(ev.at)}</td>
              <td class="prov">
                {ev.provider}
                {#if ev.host}<span class="host">{ev.host}</span>{/if}
              </td>
              <td>
                <span class="badge limited">{ev.status ?? $t('settings.rate.bodyNote')}</span>
              </td>
              <td class="detail">{ev.detail}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</div>

<style>
  .section {
    max-width: 960px;
  }
  .head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  h2 {
    margin: 0;
    font-size: 1.1rem;
    color: var(--text);
  }
  h3 {
    margin: var(--space-6) 0 var(--space-2);
    font-size: 0.95rem;
    color: var(--text);
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .win {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-size: 0.78rem;
    color: var(--muted);
  }
  .tablewrap {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow-x: auto;
    margin-top: var(--space-3);
  }
  /* ~20 rows visible, then scroll */
  .tablewrap.events {
    max-height: 640px;
    overflow-y: auto;
  }
  .tablewrap.events thead th {
    position: sticky;
    top: 0;
    background: var(--surface);
    z-index: 1;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.82rem;
  }
  th,
  td {
    padding: 6px 10px;
    border-bottom: 1px solid var(--border);
    text-align: left;
    vertical-align: top;
  }
  th {
    color: var(--muted);
    font-weight: 600;
    font-size: 0.74rem;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    white-space: nowrap;
  }
  tbody tr:last-child td {
    border-bottom: none;
  }
  tr.hot {
    background: color-mix(in srgb, var(--amber) 8%, transparent);
  }
  .num {
    text-align: right;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .strong {
    color: var(--text);
    font-weight: 600;
  }
  .prov {
    color: var(--text);
    font-weight: 600;
    white-space: nowrap;
  }
  .host {
    display: block;
    color: var(--muted);
    font-weight: 400;
    font-size: 0.72rem;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  }
  .time {
    color: var(--muted);
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
    font-size: 0.78rem;
  }
  .limit,
  .detail {
    color: var(--muted);
    font-size: 0.78rem;
    min-width: 220px;
  }
  .badge {
    display: inline-block;
    padding: 1px 7px;
    border-radius: var(--radius);
    font-size: 0.72rem;
    font-variant-numeric: tabular-nums;
  }
  .badge.limited {
    background: color-mix(in srgb, var(--amber) 22%, transparent);
    color: var(--amber);
  }
  .badge.err-badge {
    background: color-mix(in srgb, var(--red) 20%, transparent);
    color: var(--red);
  }
  .muted {
    color: var(--muted);
  }
  .small {
    font-size: 0.78rem;
  }
  .err {
    color: var(--red);
    font-size: 0.85rem;
  }
</style>
