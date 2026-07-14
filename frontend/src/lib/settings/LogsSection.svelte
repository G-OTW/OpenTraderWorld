<script>
  // App logs viewer. Logs are persisted by a tracing layer into app_logs. Filter by
  // minimum criticity and a text search; change the captured log level at runtime; clear.
  import { onMount } from 'svelte';
  import { settingsApi } from '$lib/settings/api.js';
  import Icon from '$lib/ui/Icon.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';

  const LEVELS = ['error', 'warn', 'info', 'debug', 'trace'];

  let logs = $state([]);
  let loading = $state(true);
  let error = $state('');

  let filterLevel = $state(''); // '' = all
  let search = $state('');

  // Whether the empty result is the filter's doing or the store's.
  const filtered = $derived(!!filterLevel || !!search.trim());
  function clearFilters() {
    filterLevel = '';
    search = '';
    reload();
  }

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

  // Clearing the log store is not undoable — ConfirmModal, not the browser's confirm().
  let confirmOpen = $state(false);

  function clear() {
    confirmOpen = true;
  }

  async function confirmClear() {
    await settingsApi.clearLogs();
    await reload();
  }

  // Deliberately the raw toLocaleString(), not fmtDateTime: a log timestamp needs its
  // seconds, and fmtDateTime drops them.
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

  <ErrorText error={error} />

  {#if loading}
    <!-- The log table has no <thead>, so the skeleton fills the body: a timestamp, a level
         badge, a target, and a message, at the widths the real rows use. -->
    <div class="logwrap" aria-busy="true">
      <table class="logs">
        <tbody>
          {#each Array.from({ length: 8 }, (_, i) => i) as i (i)}
            <tr>
              <td class="time"><Skeleton height="0.8rem" width="130px" /></td>
              <td class="lvl"><Skeleton height="0.8rem" width="44px" /></td>
              <td class="target"><Skeleton height="0.8rem" width="90px" /></td>
              <td class="msg"><Skeleton height="0.8rem" width="80%" /></td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else if !logs.length}
    <!-- "No logs match." was shown for both an empty store and an over-narrow filter. On a
         fresh instance nothing was filtered, so the sentence was false. Two states, two
         answers: widen the filter, or wait for the app to log something. -->
    {#if filtered}
      <EmptyState icon="search" compact title={$t('settings.logs.noMatch')}>
        {#snippet action()}
          <button class="ghost" onclick={clearFilters}>{$t('common.clear')}</button>
        {/snippet}
      </EmptyState>
    {:else}
      <EmptyState
        icon="file-text"
        compact
        title={$t('settings.logs.empty')}
        description={$t('settings.logs.emptyHint')}
      />
    {/if}
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

<ConfirmModal
  bind:open={confirmOpen}
  title={$t('settings.logs.title')}
  message={$t('settings.logs.clearConfirm')}
  confirmLabel={$t('common.clear')}
  cancelLabel={$t('common.cancel')}
  danger
  onconfirm={confirmClear}
/>

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
    font-size: var(--text-md);
    color: var(--text);
  }
  .capture {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
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
    font-size: var(--text-xs);
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
    font-size: var(--text-sm);
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
    font-size: var(--text-xs);
  }
  .msg {
    color: var(--text);
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: var(--text-xs);
    word-break: break-word;
  }
  /* Level badges follow the shared `.badge` recipe: a 10% tint of the hue plus its --*-ink.
     The old 18–22% tint was dark enough that no ink could clear 4.5:1 on it (measured 4.0:1
     even with the ink), and the raw hue as text was 2.5–3.5:1 in the light theme. */
  .badge.error {
    background: color-mix(in srgb, var(--red) 10%, transparent);
    color: var(--red-ink);
  }
  .badge.warn {
    background: color-mix(in srgb, var(--amber) 10%, transparent);
    color: var(--amber-ink);
  }
  .badge.info {
    background: color-mix(in srgb, var(--green) 10%, transparent);
    color: var(--green-ink);
  }
  .muted {
    color: var(--muted);
  }
  .small {
    font-size: var(--text-sm);
  }
</style>
