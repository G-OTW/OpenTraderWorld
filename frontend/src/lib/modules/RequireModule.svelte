<script>
  // Dependency guard. When the current module declares `requires` in the registry and one
  // of those prerequisite modules is not installed, this blocks the wrapped content and
  // shows a uniform alert with a one-click install for each missing prerequisite (plus a
  // Back button to abort). Once every prerequisite is installed the children render and the
  // task continues in place. When requirements are met (or the installed set hasn't loaded
  // yet) it renders the children unchanged.
  import { installedIds, installModule, ensureInstalled } from './installed.js';
  import { missingRequirements } from './registry.js';
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';

  // `module` = the id of the module whose page this guards. `children` = the gated content.
  let { module, children } = $props();

  const missing = $derived(missingRequirements(module, $installedIds));

  let busy = $state(null); // id currently installing
  let error = $state('');

  // Make sure the installed set is loaded so the guard reflects reality (pages may render
  // before the layout has fetched it).
  $effect(() => {
    ensureInstalled().catch(() => {});
  });

  async function install(id) {
    error = '';
    busy = id;
    try {
      await installModule(id);
    } catch (e) {
      error = e.message;
    } finally {
      busy = null;
    }
  }

  function back() {
    if (history.length > 1) history.back();
    else location.assign('/');
  }
</script>

{#if missing.length}
  <div class="require">
    <div class="card">
      <span class="glyph"><Icon name="download" size={26} /></span>
      <h2>{$t('require.title')}</h2>
      <p class="lead">{$t('require.lead')}</p>
      <ul class="deps">
        {#each missing as dep (dep.id)}
          <li>
            <span class="dep">
              <Icon name={dep.icon} size={16} />
              <span class="dname">{dep.name}</span>
              {#if dep.descKey}<span class="ddesc">{$t(dep.descKey)}</span>{/if}
            </span>
            <button class="primary" disabled={busy === dep.id} onclick={() => install(dep.id)}>
              {busy === dep.id ? $t('require.installing') : $t('require.install')}
            </button>
          </li>
        {/each}
      </ul>
      {#if error}<p class="err">{error}</p>{/if}
      <button class="ghost" onclick={back}>{$t('require.back')}</button>
    </div>
  </div>
{:else}
  {@render children?.()}
{/if}

<style>
  .require {
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: var(--space-4);
  }
  .card {
    width: 100%;
    max-width: 440px;
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: var(--space-3);
    padding: var(--space-8) var(--space-6);
    border: 0.5px solid var(--border);
    border-radius: 0;
    background: var(--surface);
    box-shadow: none;
  }
  .glyph {
    color: var(--accent);
    opacity: 0.9;
  }
  h2 {
    font-size: var(--text-lg);
    font-weight: var(--fw-medium);
    letter-spacing: 0.01em;
  }
  .lead {
    color: var(--dim);
    font-size: var(--text-base);
    line-height: 1.4;
  }
  .deps {
    list-style: none;
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    margin: var(--space-2) 0 0;
    padding: 0;
  }
  .deps li {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    text-align: left;
    background: var(--surface-2);
    border: 0.5px solid var(--border);
    border-radius: 0;
    padding: var(--space-2) var(--space-3);
  }
  .dep {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
    flex: 1;
    color: var(--text);
  }
  .dname {
    font-weight: var(--fw-medium);
    font-size: var(--text-base);
  }
  .ddesc {
    color: var(--dim);
    font-size: var(--text-xs);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* .primary / .ghost inherit the app-wide button styling from components.css. */
  .err {
    color: var(--red);
    font-size: var(--text-sm);
  }
</style>
