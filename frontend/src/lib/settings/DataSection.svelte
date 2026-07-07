<script>
  // Manage data: per-module storage size + row counts, total database size, and a
  // per-module wipe (TRUNCATE … CASCADE) gated behind a typed confirmation.
  import { onMount } from 'svelte';
  import { settingsApi, fmtBytes } from '$lib/settings/api.js';
  import { t } from '$lib/i18n';

  let usage = $state(null);
  let loading = $state(true);
  let error = $state('');

  let confirming = $state(null); // module being confirmed
  let confirmText = $state('');
  let wiping = $state(false);

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

  const sorted = $derived(
    usage ? [...usage.modules].sort((a, b) => b.size_bytes - a.size_bytes) : []
  );
  const system = $derived(
    usage?.system ? [...usage.system].sort((a, b) => b.size_bytes - a.size_bytes) : []
  );

  function askWipe(m) {
    confirming = m;
    confirmText = '';
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

  {#if error}<p class="err">{error}</p>{/if}

  {#if loading}
    <p class="muted">{$t('common.loading')}</p>
  {:else}
    <table class="tbl">
      <thead>
        <tr><th>{$t('settings.data.colModule')}</th><th class="num">{$t('settings.data.colRows')}</th><th class="num">{$t('settings.data.colSize')}</th><th></th></tr>
      </thead>
      <tbody>
        {#each sorted as m (m.id)}
          <tr>
            <td>{m.name}</td>
            <td class="num">{m.rows.toLocaleString()}</td>
            <td class="num">{fmtBytes(m.size_bytes)}</td>
            <td class="num">
              <button class="link danger" onclick={() => askWipe(m)}>{$t('settings.data.wipe')}</button>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>

    {#if system.length}
      <h3 class="sub">{$t('settings.data.system')}</h3>
      <table class="tbl">
        <thead>
          <tr><th>{$t('settings.data.colTable')}</th><th class="num">{$t('settings.data.colRows')}</th><th class="num">{$t('settings.data.colSize')}</th><th></th></tr>
        </thead>
        <tbody>
          {#each system as s (s.id)}
            <tr>
              <td>{s.name}</td>
              <td class="num">{s.rows.toLocaleString()}</td>
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
  <div
    class="overlay"
    role="presentation"
    onclick={() => (confirming = null)}
  >
    <div
      class="dialog"
      role="dialog"
      aria-modal="true"
      aria-label={$t('settings.data.wipeTitle', { name: confirming.name })}
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => e.key === 'Escape' && (confirming = null)}
    >
      <h3>{$t('settings.data.wipeConfirmTitle', { name: confirming.name })}</h3>
      <p class="muted small">
        {$t('settings.data.wipeConfirmBody', {
          name: confirming.name,
          rows: confirming.rows.toLocaleString()
        })}
      </p>
      <!-- svelte-ignore a11y_autofocus -->
      <input bind:value={confirmText} autofocus placeholder={confirming.name} />
      <div class="actions">
        <button class="ghost" onclick={() => (confirming = null)}>{$t('common.cancel')}</button>
        <button
          class="primary danger"
          disabled={confirmText !== confirming.name || wiping}
          onclick={doWipe}
        >
          {wiping ? $t('settings.data.wiping') : $t('settings.data.wipeData')}
        </button>
      </div>
    </div>
  </div>
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
    font-size: 1.1rem;
    color: var(--text);
  }
  .total {
    font-size: 0.82rem;
    color: var(--muted);
  }
  .sub {
    margin: var(--space-6) 0 0;
    font-size: 0.78rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
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

  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 50;
  }
  .dialog {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg, var(--radius));
    padding: var(--space-4);
    width: 420px;
    max-width: 90vw;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .dialog h3 {
    margin: 0;
    color: var(--text);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }
</style>
