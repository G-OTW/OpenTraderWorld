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
  import GlobalSearch from '$lib/ui/GlobalSearch.svelte';
  import NotifBell from '$lib/modules/remindme/NotifBell.svelte';
  import ServiceStatus from '$lib/ui/ServiceStatus.svelte';
  import ToastBandeau from '$lib/modules/remindme/ToastBandeau.svelte';
  import ToastHost from '$lib/ui/ToastHost.svelte';
  import { notifStore } from '$lib/modules/remindme/store.svelte.js';
  import { ensureInstalled, installedIds } from '$lib/modules/installed.js';
  import { moduleForPath } from '$lib/modules/registry';
  import ThemeToggle from '$lib/ui/ThemeToggle.svelte';
  import { theme } from '$lib/theme/store.svelte.js';
  import { accent } from '$lib/theme/accent.svelte.js';
  import { settingsApi } from '$lib/settings/api.js';

  // Routes that render without any chrome (full-screen forms). The design-system
  // gallery (/dev/ui) also renders chrome-less as a review surface.
  const bare = $derived(
    ['/setup', '/login', '/change-password', '/dev/ui'].includes($page.url.pathname)
  );

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
    // Re-assert the persisted theme against the DOM (app.html already applied it
    // pre-paint; this keeps the store and <html> in sync for runtime changes).
    theme.init();
    // Re-assert the cached app-accent (app.html applied it pre-paint); the backend value
    // is adopted below once the session is confirmed.
    accent.init();
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
      // Adopt the persisted app-accent from the backend (source of truth across devices).
      settingsApi
        .getDefaults()
        .then((d) => accent.hydrate(d.accent))
        .catch(() => {});
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

<!-- App-wide status toasts (generic UI feedback); renders on every route, chrome or not. -->
<ToastHost />

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
        <Icon name="grid" />
        <span class="navlabel">{$t('nav.dashboard')}</span>
      </button>
      <div class="search-slot">
        <GlobalSearch />
      </div>
      <ThemeToggle />
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
      radial-gradient(1100px 480px at 12% -8%, var(--glow), transparent 60%),
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
    z-index: var(--z-sticky);
    border-bottom: 1px solid var(--border);
    /* Slight glass: content scrolling under the bar reads through it. */
    background: color-mix(in srgb, var(--surface) 84%, transparent);
    backdrop-filter: blur(10px);
    -webkit-backdrop-filter: blur(10px);
  }
  .switch-slot {
    width: 240px;
  }
  /* Global search sits centered in the bar's flexible middle. */
  .search-slot {
    flex: 1;
    min-width: 0;
    display: flex;
    justify-content: center;
    padding: 0 var(--space-4);
  }

  /* One nav control, two shapes: .navlink carries a label, .cog is square and
     icon-only. Their resting/hover/active treatment is identical, so it lives in
     one rule — the active state is the ONLY place the top bar spends --accent. */
  .navlink,
  .cog {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    height: 32px;
    border: 1px solid transparent;
    border-radius: var(--radius);
    background: transparent;
    color: var(--muted);
    font-family: inherit;
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .navlink {
    gap: var(--space-2);
    padding: 0 var(--space-3);
  }
  .cog {
    width: 32px;
    line-height: 1;
  }

  .navlink:hover,
  .cog:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .navlink.active,
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

  /* Tablet: the bar runs out of room before the page does. Let the switcher
     shrink, then drop the dashboard label (its icon still says "grid"). The
     status indicator goes last — it's ambient, not actionable. */
  @media (max-width: 900px) {
    .switch-slot {
      width: auto;
      min-width: 0;
      flex: 1;
    }
    .topbar {
      gap: var(--space-2);
    }
    .navlabel {
      display: none;
    }
  }
  @media (max-width: 640px) {
    .navlink {
      padding: 0 var(--space-2);
    }
  }

  .bare {
    min-height: 100vh;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: var(--space-6);
  }
</style>
