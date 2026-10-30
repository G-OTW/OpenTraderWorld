<script>
  // Manage data: per-module storage size + row counts, total database size, and a
  // per-module wipe (TRUNCATE … CASCADE) gated behind a typed confirmation, and for
  // modules made of event-like rows a clean that deletes only rows older than a cutoff.
  import { onMount } from 'svelte';
  import { settingsApi, fmtBytes } from '$lib/settings/api.js';
  import { fmtNum } from '$lib/format.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';

  let usage = $state(null);
  let loading = $state(true);
  let error = $state('');

  let confirming = $state(null); // module being confirmed
  let confirmText = $state('');
  let wiping = $state(false);

  let cleaning = $state(null); // module being cleaned
  let cleanAmount = $state(6);
  let cleanUnit = $state('months'); // 'days' | 'weeks' | 'months' | 'years'
  let cleanBusy = $state(false);
  let cleaned = $state(''); // result line of the last clean
  const CLEAN_UNITS = ['days', 'weeks', 'months', 'years'];
  const cleanValid = $derived(Number.isInteger(cleanAmount) && cleanAmount >= 1 && cleanAmount <= 10000);

  onMount(reload);

  async function reload() {
    loading = true;
    try {
      usage = await settingsApi.dataUsage();
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  let search = $state('');
  // Biggest first is the useful default; the headers flip key and direction.
  let sortKey = $state('size'); // 'size' | 'name'
  let sortDir = $state(-1);

  function toggleSort(key) {
    if (sortKey === key) sortDir = -sortDir;
    else (sortKey = key), (sortDir = key === 'name' ? 1 : -1);
  }

  function arrange(rows) {
    const q = search.trim().toLowerCase();
    const out = q ? rows.filter((r) => r.name.toLowerCase().includes(q)) : [...rows];
    return out.sort((a, b) =>
      sortKey === 'name'
        ? a.name.localeCompare(b.name) * sortDir
        : (a.size_bytes - b.size_bytes) * sortDir
    );
  }

  const sorted = $derived(usage ? arrange(usage.modules) : []);
  const system = $derived(usage?.system ? arrange(usage.system) : []);

  function askWipe(m) {
    confirming = m;
    confirmText = '';
  }

  function askClean(m) {
    cleaning = m;
    cleaned = '';
  }

  async function doClean() {
    if (!cleaning || !cleanValid) return;
    cleanBusy = true;
    try {
      const r = await settingsApi.cleanModule(cleaning.id, cleanAmount, cleanUnit);
      cleaned = $t('settings.data.cleanDone', { rows: fmtNum(r.deleted, 0), name: r.module });
      cleaning = null;
      await reload();
    } catch (e) {
      error = e.message;
    } finally {
      cleanBusy = false;
    }
  }

  async function doWipe() {
    if (!confirming || confirmText !== confirming.name) return;
    wiping = true;
    try {
      await settingsApi.wipeModule(confirming.id);
      confirming = null;
      await reload();
    } catch (e) {
      error = e.message;
    } finally {
      wiping = false;
    }
  }
</script>

<div class="section">
  <div class="head">
    <h2>{$t('settings.data.title')}</h2>
    {#if usage}
      <span class="total">{$t('settings.data.total')} <strong>{fmtBytes(usage.database_bytes)}</strong></span>
    {/if}
  </div>
  <p class="muted small">{$t('settings.data.subtitle')}</p>

  <ErrorText error={error} />
  {#if cleaned}<p class="muted small">{cleaned}</p>{/if}

  <input
    class="search"
    type="search"
    placeholder={$t('settings.data.searchPlaceholder')}
    aria-label={$t('settings.data.searchPlaceholder')}
    bind:value={search}
  />

  <!-- The header is known before the fetch returns, so it renders immediately and only the
       body is skeletoned. Replacing the whole table with a line of text would collapse the
       column widths and snap them back. -->
  <table class="tbl" aria-busy={loading ? 'true' : undefined}>
    <thead>
      <tr><th><button class="sort" onclick={() => toggleSort('name')}>{$t('settings.data.colModule')}{#if sortKey === 'name'}<Icon name={sortDir === 1 ? 'chevron-up' : 'chevron-down'} size={11} />{/if}</button></th><th class="num">{$t('settings.data.colRows')}</th><th class="num"><button class="sort num" onclick={() => toggleSort('size')}>{$t('settings.data.colSize')}{#if sortKey === 'size'}<Icon name={sortDir === 1 ? 'chevron-up' : 'chevron-down'} size={11} />{/if}</button></th><th></th></tr>
    </thead>
    <tbody>
      {#if loading}
        {#each Array.from({ length: 5 }, (_, i) => i) as i (i)}
          <tr>
            <td><Skeleton height="0.9rem" width="55%" /></td>
            <td class="num"><Skeleton height="0.9rem" width="60%" /></td>
            <td class="num"><Skeleton height="0.9rem" width="60%" /></td>
            <td class="num"><Skeleton height="0.9rem" width="40%" /></td>
          </tr>
        {/each}
      {:else if !sorted.length}
        <tr><td colspan="4" class="muted small">{$t('settings.data.noMatch')}</td></tr>
      {:else}
        {#each sorted as m (m.id)}
          <tr>
            <td>{m.name}</td>
            <td class="num">{fmtNum(m.rows, 0)}</td>
            <td class="num">{fmtBytes(m.size_bytes)}</td>
            <td class="num row-actions">
              {#if m.cleanable}
                <button class="link" onclick={() => askClean(m)}>{$t('settings.data.clean')}</button>
              {/if}
              <button class="link danger" onclick={() => askWipe(m)}>{$t('settings.data.wipe')}</button>
            </td>
          </tr>
        {/each}
      {/if}
    </tbody>
  </table>

  {#if !loading}
    {#if system.length}
      <h3 class="sub">{$t('settings.data.system')}</h3>
      <table class="tbl">
        <thead>
          <tr><th><button class="sort" onclick={() => toggleSort('name')}>{$t('settings.data.colTable')}{#if sortKey === 'name'}<Icon name={sortDir === 1 ? 'chevron-up' : 'chevron-down'} size={11} />{/if}</button></th><th class="num">{$t('settings.data.colRows')}</th><th class="num"><button class="sort num" onclick={() => toggleSort('size')}>{$t('settings.data.colSize')}{#if sortKey === 'size'}<Icon name={sortDir === 1 ? 'chevron-up' : 'chevron-down'} size={11} />{/if}</button></th><th></th></tr>
        </thead>
        <tbody>
          {#each system as s (s.id)}
            <tr>
              <td>{s.name}</td>
              <td class="num">{fmtNum(s.rows, 0)}</td>
              <td class="num">{fmtBytes(s.size_bytes)}</td>
              <td class="num muted small">
                {#if s.id === 'app_logs'}{$t('settings.data.clearInLogs')}{/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  {/if}
</div>

{#if confirming}
  <Modal open={true} title={$t('settings.data.wipeConfirmTitle', { name: confirming.name })} fit onclose={() => (confirming = null)}>
    <div class="dialog">
      <p class="muted small">
        {$t('settings.data.wipeConfirmBody', {
          name: confirming.name,
          rows: fmtNum(confirming.rows, 0)
        })}
      </p>
      <!-- svelte-ignore a11y_autofocus -->
      <input bind:value={confirmText} autofocus placeholder={confirming.name} />
    </div>
    {#snippet footer()}
      <button class="ghost" onclick={() => (confirming = null)}>{$t('common.cancel')}</button>
      <button
        class="primary danger"
        disabled={confirmText !== confirming.name || wiping}
        onclick={doWipe}
      >
        {wiping ? $t('settings.data.wiping') : $t('settings.data.wipeData')}
      </button>
    {/snippet}
  </Modal>
{/if}

{#if cleaning}
  <Modal open={true} title={$t('settings.data.cleanTitle', { name: cleaning.name })} fit onclose={() => (cleaning = null)}>
    <div class="dialog">
      <p class="muted small">{$t(`settings.data.cleanScope.${cleaning.id}`)}</p>
      <label class="cutoff">
        <span>{$t('settings.data.cleanOlderThan')}</span>
        <!-- svelte-ignore a11y_autofocus -->
        <input type="number" min="1" max="10000" step="1" bind:value={cleanAmount} autofocus />
        <select bind:value={cleanUnit} aria-label={$t('settings.data.cleanUnit')}>
          {#each CLEAN_UNITS as u (u)}
            <option value={u}>{$t(`settings.data.unit.${u}`)}</option>
          {/each}
        </select>
      </label>
      <p class="warn small">{$t('settings.data.cleanWarn')}</p>
    </div>
    {#snippet footer()}
      <button class="ghost" onclick={() => (cleaning = null)}>{$t('common.cancel')}</button>
      <button class="primary danger" disabled={!cleanValid || cleanBusy} onclick={doClean}>
        {cleanBusy ? $t('settings.data.cleaning') : $t('settings.data.cleanData')}
      </button>
    {/snippet}
  </Modal>
{/if}

<style>
  .section {
    max-width: 680px;
  }
  .head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-3);
  }
  h2 {
    margin: 0;
    font-size: 13.5px;
    font-weight: var(--fw-medium);
    letter-spacing: 0.02em;
    color: var(--text);
  }
  .total {
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .sub {
    margin: var(--space-6) 0 0;
    font-size: var(--text-sm);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
  }
  .muted {
    color: var(--dim);
  }
  .small {
    font-size: 11.5px;
  }
  .search {
    width: 100%;
    max-width: 280px;
    margin-top: var(--space-3);
  }
  .sort {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    background: none;
    border: none;
    padding: 0;
    color: inherit;
    font: inherit;
    text-transform: inherit;
    letter-spacing: inherit;
    cursor: pointer;
  }
  .sort:hover {
    color: var(--text);
  }
  .sort.num {
    justify-content: flex-end;
    width: 100%;
  }

  .dialog {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .dialog p {
    margin: 0;
  }
  .row-actions {
    white-space: nowrap;
  }
  .row-actions .link + .link {
    margin-left: var(--space-3);
  }
  .cutoff {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
    color: var(--text);
  }
  .cutoff input {
    width: 90px;
  }
  .warn {
    margin: 0;
    color: var(--red);
  }
  /* Phone: tighter cells, a long module name wraps, and a size never splits from its unit. */
  @media (max-width: 600px) {
    .head {
      flex-wrap: wrap;
    }
    .search {
      max-width: none;
    }
    .tbl th,
    .tbl td {
      padding-left: var(--space-1);
      padding-right: var(--space-1);
    }
    .tbl td:first-child {
      overflow-wrap: anywhere;
    }
    .tbl td.num {
      white-space: nowrap;
    }
    .tbl td.row-actions {
      white-space: normal;
    }
    .row-actions .link {
      min-height: 44px;
    }
  }
</style>
