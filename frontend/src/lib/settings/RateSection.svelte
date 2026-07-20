<script>
  // API rate dashboard. Shows per-provider outbound-call volume for a window (today by
  // default) alongside each provider's published limit, plus a list of the most recent
  // over-limit responses. Observe-only: the backend never throttles — this just surfaces
  // usage and flags rate-limit answers so you can spot when a free tier is exhausted.
  import { onMount } from 'svelte';
  import { settingsApi } from '$lib/settings/api.js';
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';
  import { fmtNum } from '$lib/format.js';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';

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

  // Deliberately the raw toLocaleString(), not fmtDateTime: a rate-limit event is timed to
  // the second, and fmtDateTime drops seconds.
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

  <ErrorText error={error} />

  <!-- The header is known before the fetch lands; only the body waits. -->
  <div class="tablewrap" aria-busy={loading ? 'true' : undefined}>
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
        {#if loading}
          {#each Array.from({ length: 4 }, (_, i) => i) as i (i)}
            <tr>
              <td><Skeleton height="0.85rem" width="60%" /></td>
              <td class="num"><Skeleton height="0.85rem" width="50%" /></td>
              <td class="num"><Skeleton height="0.85rem" width="40%" /></td>
              <td class="num"><Skeleton height="0.85rem" width="40%" /></td>
              <td><Skeleton height="0.85rem" width="80%" /></td>
              <td><Skeleton height="0.85rem" width="55%" /></td>
            </tr>
          {/each}
        {:else}
          {#each providers as p (p.provider)}
            <tr class:hot={p.limited > 0}>
              <td class="prov">
                {p.provider}
                {#if p.host}<span class="host">{p.host}</span>{/if}
              </td>
              <td class="num strong">{fmtNum(p.requests, 0)}</td>
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
        {/if}
      </tbody>
    </table>
  </div>
  {#if !loading && !providers.length}
    <!-- No provider has been called yet — nothing to filter, so no action to offer. -->
    <EmptyState icon="bar-chart" compact title={$t('settings.rate.noData')} />
  {/if}

  <h3>{$t('settings.rate.eventsTitle')}</h3>
  {#if !loading && !events.length}
    <!-- No rate-limit event is the good outcome, not an empty shelf. -->
    <EmptyState icon="check-circle" compact title={$t('settings.rate.noEvents')} />
  {:else}
    <div class="tablewrap events" aria-busy={loading ? 'true' : undefined}>
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
          {#if loading}
            {#each Array.from({ length: 3 }, (_, i) => i) as i (i)}
              <tr>
                <td><Skeleton height="0.85rem" width="80%" /></td>
                <td><Skeleton height="0.85rem" width="60%" /></td>
                <td><Skeleton height="0.85rem" width="45%" /></td>
                <td><Skeleton height="0.85rem" width="70%" /></td>
              </tr>
            {/each}
          {:else}
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
          {/if}
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
    font-size: 13.5px;
    font-weight: var(--fw-medium);
    letter-spacing: 0.02em;
    color: var(--text);
  }
  h3 {
    margin: var(--space-6) 0 var(--space-2);
    font-size: 13.5px;
    font-weight: var(--fw-medium);
    letter-spacing: 0.02em;
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
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .tablewrap {
    border: var(--hairline) solid var(--border);
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
    z-index: var(--z-sticky);
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }
  th,
  td {
    padding: 6px 10px;
    border-bottom: var(--hairline) solid var(--border);
    text-align: left;
    vertical-align: top;
  }
  th {
    color: var(--dim);
    font-weight: var(--fw-medium);
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
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
    font-weight: var(--fw-medium);
  }
  .prov {
    color: var(--text);
    font-weight: var(--fw-medium);
    white-space: nowrap;
  }
  .host {
    display: block;
    color: var(--muted);
    font-weight: var(--fw-normal);
    font-size: var(--text-xs);
    font-family: var(--mono);
  }
  .time {
    color: var(--muted);
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
    font-size: var(--text-sm);
  }
  .limit,
  .detail {
    color: var(--muted);
    font-size: var(--text-sm);
    min-width: 220px;
  }
  .badge {
    display: inline-block;
    padding: 1px 7px;
    border-radius: var(--radius);
    font-size: var(--text-xs);
    font-variant-numeric: tabular-nums;
  }
  /* 10% tint, matching the shared `.badge` recipe the --*-ink values were computed
     against. At the old 20–22% the tint was dark enough that even the ink measured only
     4.0:1 in the light theme, and the raw hue as text was 2.5:1. */
  .badge.limited {
    background: color-mix(in srgb, var(--amber) 10%, transparent);
    color: var(--amber-ink);
  }
  .badge.err-badge {
    background: color-mix(in srgb, var(--red) 10%, transparent);
    color: var(--red-ink);
  }
  .muted {
    color: var(--dim);
  }
  .small {
    font-size: 11.5px;
  }
</style>
