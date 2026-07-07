<script>
  // App logs viewer. Logs are persisted by a tracing layer into app_logs. Filter by
  // minimum criticity and a text search; change the captured log level at runtime; clear.
  import { onMount } from 'svelte';
  import { settingsApi } from '$lib/settings/api.js';
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';

  const LEVELS = ['error', 'warn', 'info', 'debug', 'trace'];

  let logs = $state([]);
  let loading = $state(true);
  let error = $state('');

  let filterLevel = $state(''); // '' = all
  let search = $state('');

  let captureLevel = $state('info');

  onMount(async () => {
    try {
      const lvl = await settingsApi.getLogLevel();
      captureLevel = lvl.level;
    } catch {
      /* ignore */
    }
    await reload();
  });

  async function reload() {
    loading = true;
    error = '';
    try {
      logs = await settingsApi.logs({
        level: filterLevel || undefined,
        search: search.trim() || undefined,
        limit: 500
      });
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  async function changeCapture(level) {
    captureLevel = level;
    try {
      await settingsApi.setLogLevel(level);
    } catch (e) {
      error = e.message;
    }
  }

  async function clear() {
    if (!confirm($t('settings.logs.clearConfirm'))) return;
    await settingsApi.clearLogs();
    await reload();
  }

  const fmtTime = (iso) => {
    try {
      return new Date(iso).toLocaleString();
    } catch {
      return iso;
    }
  };
</script>

<div class="section">
  <div class="head">
    <h2>{$t('settings.logs.title')}</h2>
    <label class="capture">
      <span>{$t('settings.logs.captureLevel')}</span>
      <select value={captureLevel} onchange={(e) => changeCapture(e.currentTarget.value)}>
        {#each LEVELS as l}<option value={l}>{l}</option>{/each}
      </select>
    </label>
  </div>
  <p class="muted small">{$t('settings.logs.subtitle')}</p>

  <div class="toolbar">
    <select bind:value={filterLevel} onchange={reload}>
      <option value="">{$t('settings.logs.allLevels')}</option>
      {#each LEVELS as l}<option value={l}>{$t('settings.logs.andWorse', { level: l })}</option>{/each}
    </select>
    <input
      class="search"
      type="search"
      placeholder={$t('settings.logs.searchPlaceholder')}
      bind:value={search}
      onkeydown={(e) => e.key === 'Enter' && reload()}
    />
    <button class="ghost" onclick={reload}><Icon name="refresh-cw" size={13} /> {$t('common.refresh')}</button>
    <div class="spacer"></div>
    <button class="ghost danger" onclick={clear}><Icon name="trash" size={13} /> {$t('common.clear')}</button>
  </div>

  {#if error}<p class="err">{error}</p>{/if}

  {#if loading}
    <p class="muted">{$t('common.loading')}</p>
  {:else if !logs.length}
    <p class="muted">{$t('settings.logs.noMatch')}</p>
  {:else}
    <div class="logwrap">
      <table class="logs">
        <tbody>
          {#each logs as l (l.id)}
            <tr>
              <td class="time">{fmtTime(l.at)}</td>
              <td class="lvl"><span class="badge {l.level}">{l.level}</span></td>
              <td class="target">{l.target}</td>
              <td class="msg">{l.message}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    <p class="muted small">{$t('settings.logs.showing', { count: logs.length })}</p>
  {/if}
</div>

<style>
  .section {
    max-width: 900px;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }
  h2 {
    margin: 0;
    font-size: 1.1rem;
    color: var(--text);
  }
  .capture {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-size: 0.78rem;
    color: var(--muted);
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: var(--space-3) 0;
    flex-wrap: wrap;
  }
  .toolbar .spacer {
    flex: 1;
  }
  .search {
    min-width: 220px;
  }
  .badge {
    text-transform: uppercase;
    letter-spacing: 0.03em;
    font-size: 0.68rem;
  }
  .logwrap {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: auto;
    max-height: 60vh;
  }
  .logs {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.8rem;
  }
  .logs td {
    padding: 5px 8px;
    border-bottom: 1px solid var(--border);
    vertical-align: top;
  }
  .time {
    color: var(--muted);
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  .target {
    color: var(--muted);
    white-space: nowrap;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 0.74rem;
  }
  .msg {
    color: var(--text);
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 0.76rem;
    word-break: break-word;
  }
  .badge.error {
    background: color-mix(in srgb, var(--red) 22%, transparent);
    color: var(--red);
  }
  .badge.warn {
    background: color-mix(in srgb, var(--amber) 22%, transparent);
    color: var(--amber);
  }
  .badge.info {
    background: color-mix(in srgb, var(--green) 18%, transparent);
    color: var(--green);
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
