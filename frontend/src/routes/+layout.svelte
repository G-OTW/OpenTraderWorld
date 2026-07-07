<script>
  import '$lib/theme/default.css';
  import '$lib/theme/components.css';
  import Icon from '$lib/ui/Icon.svelte';
  import { t, initLocale } from '$lib/i18n';
  import { api } from '$lib/api';
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import { onMount } from 'svelte';
  import ModuleSwitcher from '$lib/modules/ModuleSwitcher.svelte';
  import NotifBell from '$lib/modules/remindme/NotifBell.svelte';
  import ServiceStatus from '$lib/ui/ServiceStatus.svelte';
  import ToastBandeau from '$lib/modules/remindme/ToastBandeau.svelte';
  import { notifStore } from '$lib/modules/remindme/store.svelte.js';
  import { ensureInstalled, installedIds } from '$lib/modules/installed.js';
  import { moduleForPath } from '$lib/modules/registry';

  // Routes that render without any chrome (full-screen forms).
  const bare = $derived(['/setup', '/login', '/change-password'].includes($page.url.pathname));

  // Browser tab title: "OTW - <page name>".
  const pageName = $derived.by(() => {
    const path = $page.url.pathname;
    if (path === '/setup') return $t('nav.setup');
    if (path === '/login') return $t('nav.login');
    if (path === '/change-password') return $t('changePassword.title');
    if (path.startsWith('/settings')) return $t('nav.settings');
    return moduleForPath(path).name;
  });

  // Guard: a detached feature module's route is inaccessible. If the current path maps to a
  // module that isn't installed, send the user to the dashboard. The home (dashboard) and
  // chrome-less routes (settings/setup/login) are never guarded.
  $effect(() => {
    const ids = $installedIds;
    if (!ids || bare) return;
    const path = $page.url.pathname;
    if (path === '/' || path.startsWith('/settings')) return;
    const mod = moduleForPath(path);
    if (!mod.home && !ids.has(mod.id)) goto('/');
  });

  onMount(async () => {
    // Apply the persisted UI language (localStorage already seeded the store on load).
    initLocale();
    try {
      const status = await api.setupStatus();
      if (!status.configured) {
        if ($page.url.pathname !== '/setup') await goto('/setup');
        return;
      }
      // Configured: require a valid session for everything except the login screen.
      if ($page.url.pathname !== '/login') {
        const authed = await api.isAuthenticated();
        if (!authed) {
          await goto('/login');
          return;
        }
      }
      // Load which modules are installed (drives the switcher + dashboard visibility).
      ensureInstalled().catch(() => {});
      // Start polling reminder notifications once we're past setup + auth.
      notifStore.start();
    } catch {
      // Unreachable core: the ServiceStatus indicator surfaces this in the top bar.
    }
  });

  let { children } = $props();
</script>

<svelte:head>
  <title>OTW - {pageName}</title>
</svelte:head>

{#if bare}
  <div class="bare">{@render children?.()}</div>
{:else}
  <div class="app">
    <!-- Global top bar: module switcher (top-left) + core status. -->
    <header class="topbar">
      <div class="switch-slot">
        <ModuleSwitcher />
      </div>
      <button
        class="navlink"
        class:active={$page.url.pathname === '/'}
        title={$t('nav.dashboard')}
        onclick={() => goto('/')}
      >
        <Icon name="grid" /> {$t('nav.dashboard')}
      </button>
      <div class="spacer"></div>
      <NotifBell />
      <button
        class="cog"
        class:active={$page.url.pathname.startsWith('/settings')}
        title={$t('nav.settings')}
        aria-label={$t('nav.settings')}
        onclick={() => goto('/settings')}
      >
        <Icon name="settings" size={17} />
      </button>
      <ServiceStatus />
    </header>

    <!-- Module context: each module renders its own sidebar + content here. -->
    <div class="module-context">
      {@render children?.()}
    </div>
  </div>

  <!-- Slide-in reminder toasts (global, above all modules). -->
  <ToastBandeau />
{/if}

<style>
  :global(*),
  :global(*::before),
  :global(*::after) {
    box-sizing: border-box;
    margin: 0;
    padding: 0;
  }
  :global(html),
  :global(body) {
    height: 100%;
  }
  :global(body) {
    font-family: var(--font);
    /* A faint accent glow behind the top-left (where the app "starts") gives the whole
       surface depth without competing with content. */
    background:
      radial-gradient(1100px 480px at 12% -8%, rgba(91, 157, 255, 0.07), transparent 60%),
      var(--bg);
    background-attachment: fixed;
    color: var(--text);
    -webkit-font-smoothing: antialiased;
  }

  .app {
    display: grid;
    grid-template-rows: var(--topbar-height) 1fr;
    height: 100vh;
  }

  .topbar {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    padding: 0 var(--space-4);
    /* Explicit stacking context above the module content: backdrop-filter alone creates
       one, but without a z-index Safari paints the bar's dropdowns behind positioned
       elements in the page (e.g. dashboard cards). */
    position: relative;
    z-index: 100;
    border-bottom: 1px solid var(--border);
    /* Slight glass: content scrolling under the bar reads through it. */
    background: color-mix(in srgb, var(--surface) 84%, transparent);
    backdrop-filter: blur(10px);
    -webkit-backdrop-filter: blur(10px);
  }
  .switch-slot {
    width: 240px;
  }
  .spacer {
    flex: 1;
  }
  .navlink {
    background: transparent;
    border: 1px solid transparent;
    color: var(--muted);
    font-size: 0.85rem;
    font-family: inherit;
    height: 32px;
    padding: 0 var(--space-3);
    border-radius: var(--radius);
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
  }
  .navlink:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .navlink.active {
    color: var(--accent);
    border-color: var(--border);
    background: var(--surface-2);
  }
  .cog {
    background: transparent;
    border: 1px solid transparent;
    color: var(--muted);
    font-size: 1.05rem;
    line-height: 1;
    width: 32px;
    height: 32px;
    border-radius: var(--radius);
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }
  .cog:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .cog.active {
    color: var(--accent);
    border-color: var(--border);
    background: var(--surface-2);
  }
  /* Module context fills the area below the top bar; modules manage their own layout. */
  .module-context {
    min-height: 0;
    overflow: hidden;
  }

  .bare {
    min-height: 100vh;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: var(--space-6);
  }
</style>
