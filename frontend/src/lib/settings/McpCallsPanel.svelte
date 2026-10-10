<script>
  // Agent call log: what agents called through the gateway, what failed and why. Read-only
  // aggregates over `mcp_calls` (remote MCP clients and the in-app assistant alike).
  import { onMount } from 'svelte';
  import { settingsApi } from '$lib/settings/api.js';
  import { t } from '$lib/i18n';
  import { fmtDateTime } from '$lib/format.js';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Icon from '$lib/ui/Icon.svelte';

  let days = $state('7');
  let stats = $state(null);
  let error = $state('');
  let loading = $state(false);

  const pct = $derived(stats?.calls ? Math.round((stats.errors / stats.calls) * 1000) / 10 : 0);
  // Classes on failed calls only: a success that carries a note is counted separately.
  const failing = $derived(
    (stats?.by_class ?? []).filter((c) => !['ignored_input', 'schema_drift', 'idempotent_replay'].includes(c.class)),
  );

  async function load() {
    loading = true;
    error = '';
    try {
      stats = await settingsApi.mcpCalls(Number(days));
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  onMount(load);
</script>

<div class="bar">
  <span class="summary">
    {#if stats}
      {$t('settings.mcp.callsTotal', { n: stats.calls })}
      · <span class:bad={stats.errors > 0}>{$t('settings.mcp.callsErrors', { n: stats.errors, pct })}</span>
      {#if stats.notes}· <span class="warn">{$t('settings.mcp.callsNotes', { n: stats.notes })}</span>{/if}
    {/if}
  </span>
  <span class="controls">
    <Dropdown
      value={days}
      onpick={(v) => {
        days = v;
        load();
      }}
      ariaLabel={$t('settings.mcp.callsTitle')}
      options={[
        { value: '1', label: $t('settings.mcp.callsDays', { n: 1 }) },
        { value: '7', label: $t('settings.mcp.callsDays', { n: 7 }) },
        { value: '30', label: $t('settings.mcp.callsDays', { n: 30 }) },
      ]}
    />
    <button class="ghost" disabled={loading} onclick={load}>
      <Icon name="refresh-cw" size={12} /> {$t('common.refresh')}
    </button>
  </span>
</div>

<ErrorText {error} />

{#if stats && !stats.calls}
  <EmptyState icon="zap" compact title={$t('settings.mcp.callsEmpty')} />
{:else if stats}
  {#if stats.by_class.length}
    <div class="chips">
      {#each failing as c (c.class)}
        <span class="chip bad">{c.class} · {c.calls}</span>
      {/each}
      {#each stats.by_class.filter((c) => !failing.includes(c)) as c (c.class)}
        <span class="chip">{c.class} · {c.calls}</span>
      {/each}
    </div>
  {/if}

  <div class="tablewrap">
    <table>
      <thead>
        <tr>
          <th>{$t('settings.mcp.colRoute')}</th>
          <th class="num">{$t('settings.mcp.colCalls')}</th>
          <th class="num">{$t('settings.mcp.colErrors')}</th>
          <th class="num">{$t('settings.mcp.colAvgMs')}</th>
        </tr>
      </thead>
      <tbody>
        {#each stats.by_route as r (r.method + r.route)}
          <tr>
            <td class="mono">{r.method} {r.route}</td>
            <td class="num">{r.calls}</td>
            <td class="num" class:bad={r.errors > 0}>{r.errors}</td>
            <td class="num">{Math.round(r.avg_ms)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>

  {#if stats.recent_failures.length}
    <h4>{$t('settings.mcp.callsRecent')}</h4>
    <div class="tablewrap">
      <table>
        <thead>
          <tr>
            <th>{$t('settings.mcp.colWhen')}</th>
            <th>{$t('settings.mcp.colRoute')}</th>
            <th>{$t('settings.mcp.colCause')}</th>
            <th>{$t('settings.mcp.colDetail')}</th>
          </tr>
        </thead>
        <tbody>
          {#each stats.recent_failures as f, i (i)}
            <tr>
              <td>{fmtDateTime(f.at)}</td>
              <td class="mono">{f.tool} {f.method} {f.route}</td>
              <td>{f.class ?? f.status}</td>
              <td class="detail" title={f.detail}>{f.detail}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
{/if}

<style>
  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    margin: var(--space-2) 0;
  }
  .summary {
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .controls {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
  }
  .bad {
    color: var(--red);
  }
  .warn {
    color: var(--amber);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
    margin-bottom: var(--space-2);
  }
  .chip {
    font-family: var(--mono);
    font-size: 11px;
    padding: 2px 8px;
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    color: var(--muted);
  }
  .chip.bad {
    color: var(--red);
  }
  h4 {
    margin: var(--space-3) 0 var(--space-2);
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
  }
  .tablewrap {
    overflow-x: auto;
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    margin-bottom: var(--space-3);
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }
  th,
  td {
    text-align: left;
    padding: 6px 10px;
    border-bottom: var(--hairline) solid var(--border);
    white-space: nowrap;
  }
  tbody tr:last-child td {
    border-bottom: none;
  }
  th {
    color: var(--dim);
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    font-weight: var(--fw-medium);
    background: var(--surface-2);
  }
  .num {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .mono {
    font-family: var(--mono);
    color: var(--muted);
  }
  .detail {
    white-space: normal;
    min-width: 260px;
    max-width: 520px;
    color: var(--muted);
  }
</style>
