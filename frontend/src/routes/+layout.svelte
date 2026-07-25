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
  import { toast } from '$lib/ui/toast.svelte.js';
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

  // Demo sandbox banner: /api/demo is public and cheap; when the backend runs with
  // OTW_DEMO=1 it returns the next quarter-hour reset so the countdown is truthful.
  let demoOn = $state(false);
  let demoResetAt = $state(null);
  let demoLeft = $state('');

  // Disclaimer, acknowledged once per browser session. The demo runs on a free tier with
  // free LLM models, so visitors must be told up front that it can be slow and wrong.
  // sessionStorage (not local): the sandbox wipes every 15 min, so a returning visitor
  // lands on fresh state and should see the warning again.
  const DEMO_ACK_KEY = 'otw.demo.ack.v1';
  let demoIntro = $state(false);

  function demoAck() {
    demoIntro = false;
    try {
      sessionStorage.setItem(DEMO_ACK_KEY, '1');
    } catch {
      /* storage blocked — the modal just shows again on the next page load */
    }
  }

  async function demoRefresh() {
    try {
      const r = await fetch('/api/demo');
      const d = await r.json();
      demoOn = !!d.demo;
      demoResetAt = d.next_reset_at ? new Date(d.next_reset_at) : null;
      if (demoOn) {
        installDemoInterceptor();
        let acked = false;
        try {
          acked = sessionStorage.getItem(DEMO_ACK_KEY) === '1';
        } catch {
          /* storage blocked — show it */
        }
        if (!acked) demoIntro = true;
      }
    } catch {
      /* not a demo host, or core unreachable — no banner */
    }
  }

  // Demo: every control stays clickable — the backend gate answers 403 `demo_disabled`
  // to anything locked. This global interceptor turns that answer into a friendly hint,
  // so visitors learn WHY instead of seeing a dead button or a raw error.
  let demoToastAt = 0;
  function installDemoInterceptor() {
    if (window.__otwDemoIntercept) return;
    window.__otwDemoIntercept = true;
    const orig = window.fetch.bind(window);
    window.fetch = async (...args) => {
      const res = await orig(...args);
      if (res.status === 403) {
        try {
          const d = await res.clone().json();
          if (d?.error === 'demo_disabled' && Date.now() - demoToastAt > 3000) {
            demoToastAt = Date.now();
            toast.warn(
              'Locked in the public demo — this shared sandbox resets every 15 minutes, so settings, connectors and imports stay read-only. Self-host OTW to unlock everything.'
            );
          }
        } catch {
          /* non-JSON 403 — not ours */
        }
      }
      return res;
    };
  }

  onMount(() => {
    demoRefresh();
    const tick = setInterval(() => {
      if (!demoOn || !demoResetAt) return;
      const ms = demoResetAt - Date.now();
      if (ms <= 0) {
        demoLeft = '0:00';
        demoRefresh();
        return;
      }
      const m = Math.floor(ms / 60000);
      const s = Math.floor((ms % 60000) / 1000);
      demoLeft = `${m}:${String(s).padStart(2, '0')}`;
    }, 1000);
    return () => clearInterval(tick);
  });
</script>

<svelte:head>
  <title>OTW - {pageName}</title>
</svelte:head>

<!-- App-wide status toasts (generic UI feedback); renders on every route, chrome or not. -->
<ToastHost />

<!-- Demo disclaimer: acknowledgment is mandatory, so this dialog has no backdrop
     dismiss, no Escape and no close button — hence not the shared Modal.svelte. -->
{#if demoIntro}
  <div class="intro-backdrop" role="presentation">
    <div class="intro" role="alertdialog" aria-modal="true" aria-labelledby="demo-intro-title">
      <header class="intro-head">
        <h3 id="demo-intro-title">Welcome to the OpenTraderWorld demo</h3>
      </header>
      <div class="intro-body">
        <p>
          This public demo runs on a <strong>free-tier server</strong> and answers with
          <strong>free AI models</strong>. Speed and accuracy are limited by both.
        </p>
        <ul>
          <li>Pages and AI answers can be <strong>slow</strong>, and the AI budget is shared, so it may be temporarily used up.</li>
          <li>AI output may be <strong>inaccurate or plainly wrong</strong> — it is not financial advice.</li>
          <li>It is a <strong>shared sandbox</strong>: everything you type is visible to other visitors and wiped every 15 minutes. Do not enter anything private.</li>
          <li>Settings, connectors and imports are read-only here.</li>
        </ul>
        <p class="intro-note">
          Self-host OpenTraderWorld to run it at full speed with your own models and data.
        </p>
      </div>
      <footer class="intro-foot">
        <button class="intro-btn" onclick={demoAck}>I understand — explore the demo</button>
      </footer>
    </div>
  </div>
{/if}

{#if demoOn}
  <div class="demo-banner" role="status">
    <strong>Public demo</strong>
    <span class="demo-sep">·</span>
    shared sandbox, all changes are visible to everyone and wiped every 15 minutes
    {#if demoLeft}
      <span class="demo-sep">·</span>
      next reset in <span class="demo-count">{demoLeft}</span>
    {/if}
    <span class="demo-sep">·</span>
    <a class="demo-link" href="https://opentraderworld.com" target="_blank" rel="noopener">
      opentraderworld.com
    </a>
  </div>
{/if}

{#if bare}
  <div class="bare">{@render children?.()}</div>
{:else}
  <div class="app" class:with-demo={demoOn}>
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
        <button
          class="cog"
          class:active={$page.url.pathname.startsWith('/agent')}
          title={$t('nav.agent')}
          aria-label={$t('nav.agent')}
          onclick={() => goto('/agent')}
        >
          <Icon name="brain" size={17} />
        </button>
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
    /* Flat institutional ground — no glow, no gradient. */
    background: var(--bg);
    color: var(--text);
    -webkit-font-smoothing: antialiased;
  }

  .app {
    display: grid;
    grid-template-rows: var(--topbar-height) 1fr;
    height: 100vh;
  }
  .app.with-demo {
    height: calc(100vh - 28px);
  }

  /* Demo disclaimer dialog — mirrors Modal.svelte's institutional styling. */
  .intro-backdrop {
    position: fixed;
    inset: 0;
    z-index: var(--z-modal);
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.6);
    backdrop-filter: blur(2px);
    padding: var(--space-4);
  }
  .intro {
    width: 100%;
    max-width: 520px;
    max-height: calc(100vh - 2 * var(--space-8));
    overflow-y: auto;
    background: var(--surface);
    border: var(--hairline) solid var(--border-control);
    border-radius: 0;
  }
  .intro-head {
    padding: var(--space-4);
    border-bottom: var(--hairline) solid var(--border);
  }
  .intro-head h3 {
    margin: 0;
    color: var(--text);
    font-size: var(--fs-item-title);
    font-weight: var(--fw-medium);
    letter-spacing: 0.02em;
  }
  .intro-body {
    padding: var(--space-4);
    color: var(--text);
    font-size: 13px;
    line-height: 1.55;
  }
  .intro-body p {
    margin: 0 0 var(--space-3);
  }
  .intro-body ul {
    margin: 0 0 var(--space-3);
    padding-left: var(--space-4);
  }
  .intro-body li {
    margin-bottom: var(--space-2);
  }
  .intro-note {
    margin: 0;
    color: var(--muted);
  }
  .intro-foot {
    display: flex;
    justify-content: flex-end;
    padding: 0 var(--space-4) var(--space-4);
  }
  .intro-btn {
    background: var(--accent);
    color: #fff;
    border: none;
    border-radius: 0;
    padding: var(--space-2) var(--space-4);
    font-size: 13px;
    cursor: pointer;
  }
  .intro-btn:hover {
    filter: brightness(1.08);
  }

  .demo-banner {
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    font-size: var(--fs-small, 12px);
    background: var(--amber);
    /* Amber band stays readable in both themes: dark text on the amber fill. */
    color: #1a1a1a;
    border-bottom: 0.5px solid var(--border);
    white-space: nowrap;
    overflow: hidden;
    padding: 0 var(--space-3);
  }
  .demo-sep {
    opacity: 0.6;
  }
  .demo-count {
    font-variant-numeric: tabular-nums;
    font-weight: 600;
  }
  .demo-link {
    color: inherit;
    font-weight: 600;
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  .demo-link:hover {
    opacity: 0.75;
  }

  .topbar {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    /* Fixed height (--topbar-height) handles vertical rhythm; the horizontal
       gutter follows --pad-header (16px 28px → 28px inline). */
    padding-inline: 28px;
    /* Explicit stacking context above the module content: backdrop-filter alone creates
       one, but without a z-index Safari paints the bar's dropdowns behind positioned
       elements in the page (e.g. dashboard cards). */
    position: relative;
    z-index: var(--z-sticky);
    border-bottom: 0.5px solid var(--border);
    background: var(--bg);
  }
  .switch-slot {
    width: 240px;
  }
  /* Global search sits centered in the bar's flexible middle, the agent button
     snug against its right edge. */
  .search-slot {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
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
    border: 0.5px solid transparent;
    border-radius: 0;
    background: transparent;
    color: var(--muted);
    font-family: inherit;
    font-size: var(--fs-body);
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
    border-color: var(--border-control);
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
