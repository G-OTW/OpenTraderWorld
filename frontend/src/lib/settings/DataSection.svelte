<script>
  // Manage data: per-module storage size + row counts, total database size, and a
  // per-module wipe (TRUNCATE … CASCADE) gated behind a typed confirmation.
  import { onMount } from 'svelte';
  import { settingsApi, fmtBytes } from '$lib/settings/api.js';
  import { fmtNum } from '$lib/format.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';

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

  <ErrorText error={error} />

  <!-- The header is known before the fetch returns, so it renders immediately and only the
       body is skeletoned. Replacing the whole table with a line of text would collapse the
       column widths and snap them back. -->
  <table class="tbl" aria-busy={loading ? 'true' : undefined}>
    <thead>
      <tr><th>{$t('settings.data.colModule')}</th><th class="num">{$t('settings.data.colRows')}</th><th class="num">{$t('settings.data.colSize')}</th><th></th></tr>
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
      {:else}
        {#each sorted as m (m.id)}
          <tr>
            <td>{m.name}</td>
            <td class="num">{fmtNum(m.rows, 0)}</td>
            <td class="num">{fmtBytes(m.size_bytes)}</td>
            <td class="num">
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
          <tr><th>{$t('settings.data.colTable')}</th><th class="num">{$t('settings.data.colRows')}</th><th class="num">{$t('settings.data.colSize')}</th><th></th></tr>
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
          rows: fmtNum(confirming.rows, 0)
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

  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: var(--z-modal);
  }
  .dialog {
    background: var(--surface);
    border: var(--hairline) solid var(--border);
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
