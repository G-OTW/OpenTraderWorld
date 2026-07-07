<script>
  // Modules: browse every available module, install (make it available in the switcher and
  // dashboard) or detach (hide it). Detaching can optionally delete all of the module's
  // data. Installing/detaching only flips a visibility flag — nothing is downloaded.
  import { onMount } from 'svelte';
  import { featureModules } from '$lib/modules/registry';
  import { installedIds, ensureInstalled, installModule, detachModule } from '$lib/modules/installed.js';
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';

  let busy = $state(null); // id currently being installed/detached
  let error = $state('');

  let detaching = $state(null); // module pending detach confirmation
  let wipeData = $state(false);

  onMount(() => {
    ensureInstalled().catch((e) => (error = e.message));
  });

  const installedCount = $derived($installedIds ? $installedIds.size : 0);

  function isInstalled(id) {
    return $installedIds?.has(id) ?? false;
  }

  async function doInstall(m) {
    busy = m.id;
    error = '';
    try {
      await installModule(m.id);
    } catch (e) {
      error = e.message;
    } finally {
      busy = null;
    }
  }

  function askDetach(m) {
    detaching = m;
    wipeData = false;
  }

  async function doDetach() {
    if (!detaching) return;
    const m = detaching;
    busy = m.id;
    error = '';
    try {
      await detachModule(m.id, wipeData);
      detaching = null;
    } catch (e) {
      error = e.message;
    } finally {
      busy = null;
    }
  }
</script>

<div class="section">
  <div class="head">
    <h2>{$t('settings.modules.title')}</h2>
    <span class="total">{$t('settings.modules.count', { count: installedCount })}</span>
  </div>
  <p class="muted small">{$t('settings.modules.subtitle')}</p>

  {#if error}<p class="err">{error}</p>{/if}

  {#if $installedIds === null}
    <p class="muted">{$t('common.loading')}</p>
  {:else}
    <ul class="list">
      {#each featureModules as m (m.id)}
        <li class:installed={isInstalled(m.id)}>
          <span class="ico"><Icon name={m.icon} size={17} /></span>
          <span class="label">
            <span class="name">{m.name}</span>
            {#if m.descKey}<span class="desc">{$t(m.descKey)}</span>{/if}
          </span>
          {#if isInstalled(m.id)}
            <span class="badge green">{$t('settings.modules.installed')}</span>
            <button class="ghost danger" disabled={busy === m.id} onclick={() => askDetach(m)}>
              {$t('settings.modules.detach')}
            </button>
          {:else}
            <button class="primary" disabled={busy === m.id} onclick={() => doInstall(m)}>
              {busy === m.id ? $t('settings.modules.installing') : $t('settings.modules.install')}
            </button>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</div>

{#if detaching}
  <div class="overlay" role="presentation" onclick={() => (detaching = null)}>
    <div
      class="dialog"
      role="dialog"
      aria-modal="true"
      aria-label={$t('settings.modules.detachTitle', { name: detaching.name })}
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => e.key === 'Escape' && (detaching = null)}
    >
      <h3>{$t('settings.modules.detachConfirmTitle', { name: detaching.name })}</h3>
      <p class="muted small">{$t('settings.modules.detachConfirmBody', { name: detaching.name })}</p>
      <label class="check">
        <input type="checkbox" bind:checked={wipeData} />
        <span>
          {$t('settings.modules.alsoDelete', { name: detaching.name })}
          <span class="muted small block">{$t('settings.modules.deleteWarn')}</span>
        </span>
      </label>
      <div class="actions">
        <button class="ghost" onclick={() => (detaching = null)}>{$t('common.cancel')}</button>
        <button class="primary danger" disabled={busy === detaching.id} onclick={doDetach}>
          {busy === detaching.id
            ? $t('settings.modules.detaching')
            : wipeData
              ? $t('settings.modules.detachDelete')
              : $t('settings.modules.detach')}
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
  .list {
    list-style: none;
    margin: var(--space-4) 0 0;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .list li {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: 10px 12px;
    border-bottom: 1px solid var(--border);
  }
  .list li:last-child {
    border-bottom: none;
  }
  .ico {
    width: 1.4rem;
    display: inline-flex;
    justify-content: center;
    color: var(--accent);
  }
  .label {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
  }
  .name {
    font-size: 0.9rem;
    font-weight: 600;
    color: var(--text);
  }
  .desc {
    font-size: 0.74rem;
    color: var(--muted);
  }
  .muted {
    color: var(--muted);
  }
  .small {
    font-size: 0.78rem;
  }
  .block {
    display: block;
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
    width: 440px;
    max-width: 90vw;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .dialog h3 {
    margin: 0;
    color: var(--text);
  }
  .check {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    font-size: 0.85rem;
    color: var(--text);
    cursor: pointer;
  }
  .check input {
    margin-top: 2px;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }
</style>
