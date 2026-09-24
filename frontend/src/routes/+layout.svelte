<script>
  import '$lib/theme/default.css';
  import '$lib/theme/components.css';
  import '$lib/theme/widgets.css';
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
  import TipHost from '$lib/ui/TipHost.svelte';
  import { toast } from '$lib/ui/toast.svelte.js';
  import { notifStore } from '$lib/modules/remindme/store.svelte.js';
  import { ensureInstalled, installedIds } from '$lib/modules/installed.js';
  import { moduleForPath, visibleModules } from '$lib/modules/registry';
  import ThemeToggle from '$lib/ui/ThemeToggle.svelte';
  import AssistantWidget from '$lib/modules/agent/AssistantWidget.svelte';
  import ScrollTop from '$lib/ui/ScrollTop.svelte';
  import ReauthModal from '$lib/ui/ReauthModal.svelte';
  import { theme } from '$lib/theme/store.svelte.js';
  import { privacy, togglePrivacy } from '$lib/theme/privacy.svelte.js';
  import { accent } from '$lib/theme/accent.svelte.js';
  import { tz } from '$lib/tz.svelte.js';
  import { settingsApi } from '$lib/settings/api.js';
  import { dashboardApi } from '$lib/modules/dashboard/api.js';
  import { DASHBOARD_PRESETS, isDashboardPreset } from '$lib/modules/dashboard/layout.js';
  import RailEditor from '$lib/ui/RailEditor.svelte';

  // Routes that render without any chrome (full-screen forms). The design-system
  // gallery (/dev/ui) also renders chrome-less as a review surface, and so does the
  // mailbox OAuth callback — a throwaway tab the user closes straight away.
  const bare = $derived(
    ['/setup', '/login', '/change-password', '/request-reset', '/dev/ui', '/mailbox/oauth'].includes(
      $page.url.pathname
    )
  );

  // Browser tab title: "OTW - <page name>".
  const pageName = $derived.by(() => {
    const path = $page.url.pathname;
    if (path === '/setup') return $t('nav.setup');
    if (path === '/login') return $t('nav.login');
    if (path === '/change-password') return $t('changePassword.title');
    if (path === '/request-reset') return $t('resetHelp.title');
    if (path.startsWith('/settings')) return $t('nav.settings');
    return moduleForPath(path).name;
  });

  // ── Navigation rail ───────────────────────────────────────────────────────
  // Two arrangeable sections: Workspace (the user's dashboards) and Tools (modules).
  // Workspace is capped at WORKSPACE_MAX, so its eight entries plus Custom + always show
  // whole. Tools is uncapped: it takes whatever height the window leaves and scrolls past
  // it. Both are arranged from the pencil in the section header.
  //
  // The arrangement rides in the dashboard document (`rail`): the Workspace entries are
  // the dashboards, so one saved document owns the list and its order.
  const WORKSPACE_MAX = 8;
  const PRESET_ICONS = {
    __preset_investor__: 'coins',
    __preset_trader__: 'trending-up',
    __preset_quant__: 'ruler',
    __preset_portfolio__: 'briefcase',
    __preset_utilities__: 'zap'
  };

  let railDoc = $state(null); // last loaded dashboard document (null until it answers)
  let railBusy = $state(false);

  // Sidebar open/closed. Collapsed means gone, not narrowed: the toggle in the top bar is
  // the only thing left of it, so the shell reads as one full-width workbar.
  const RAIL_OPEN_KEY = 'otw.rail.open';
  let railOpen = $state(true);

  function toggleRail() {
    railOpen = !railOpen;
    try {
      localStorage.setItem(RAIL_OPEN_KEY, railOpen ? '1' : '0');
    } catch {
      /* storage blocked - the choice just doesn't survive a reload */
    }
  }
  let wsEditOpen = $state(false);
  let toolsEditOpen = $state(false);

  function loadRail() {
    dashboardApi
      .getLayout()
      .then((saved) => {
        railDoc = saved ?? null;
      })
      .catch(() => {
        /* the dashboard page surfaces load failures; the rail just shows presets */
      });
  }

  // Every dashboard that could sit in Workspace: the seeded presets first, then the
  // user's own pages. Before the document answers, the presets alone keep the rail whole.
  const workspaceCandidates = $derived.by(() => {
    const pages = railDoc?.pages ?? [];
    const seen = new Set();
    const out = [];
    for (const preset of DASHBOARD_PRESETS) {
      const saved = pages.find((p) => p.id === preset.id);
      seen.add(preset.id);
      out.push({ id: preset.id, name: saved?.tag || saved?.name || preset.name, icon: PRESET_ICONS[preset.id] ?? 'grid' });
    }
    for (const p of pages) {
      if (seen.has(p.id) || isDashboardPreset(p.id)) continue;
      out.push({ id: p.id, name: p.tag || p.name, icon: 'grid' });
    }
    return out;
  });

  // Candidate tools: every installed module that isn't the dashboard itself.
  const toolCandidates = $derived(
    visibleModules($installedIds)
      .filter((module) => !module.home)
      .map((module) => ({ id: module.id, name: module.name, icon: module.icon, base: module.base }))
  );

  // A section's entries: the saved arrangement filtered to what still exists, else the
  // section default. `max` is Infinity for an uncapped section.
  function railSection(candidates, saved, fallbackIds, max) {
    const byId = new Map(candidates.map((c) => [c.id, c]));
    const ids = Array.isArray(saved) ? saved : fallbackIds;
    const entries = ids.map((id) => byId.get(id)).filter(Boolean);
    return Number.isFinite(max) ? entries.slice(0, max) : entries;
  }

  const workspaceEntries = $derived(
    railSection(
      workspaceCandidates,
      railDoc?.rail?.workspace,
      DASHBOARD_PRESETS.map((preset) => preset.id),
      WORKSPACE_MAX
    )
  );
  // No saved arrangement: every installed tool is listed, in registry order.
  const toolEntries = $derived(
    railSection(
      toolCandidates,
      railDoc?.rail?.tools,
      toolCandidates.map((tool) => tool.id),
      Infinity
    )
  );

  // Persist one section. Re-reads the document first: the dashboard page writes the same
  // one, and the rail must not push back a stale layout over it.
  async function saveRail(section, ids) {
    railBusy = true;
    try {
      const fresh = (await dashboardApi.getLayout()) ?? {};
      fresh.rail = { ...(fresh.rail ?? {}), [section]: ids };
      await dashboardApi.saveLayout(fresh);
      railDoc = fresh;
    } catch (e) {
      toast.err(e.message);
    } finally {
      railBusy = false;
    }
  }

  // Which entry is lit: the `?dashboard=` target on the home route, else the page the
  // dashboard opens to (its first favourite).
  const selectedDashboard = $derived.by(() => {
    if ($page.url.pathname !== '/') return null;
    const requested = $page.url.searchParams.get('dashboard');
    if (requested) {
      const preset = `__preset_${requested}__`;
      return isDashboardPreset(preset) ? preset : requested;
    }
    return railDoc?.favoriteIds?.[0] ?? DASHBOARD_PRESETS[0].id;
  });

  // Presets keep their short slug in the URL; user pages travel by id.
  function dashboardHref(id) {
    const slug = isDashboardPreset(id) ? id.replace(/^__preset_|__$/g, '') : id;
    return `/?dashboard=${encodeURIComponent(slug)}`;
  }

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
      railOpen = localStorage.getItem(RAIL_OPEN_KEY) !== '0';
    } catch {
      /* storage blocked - default to open */
    }
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
      // Configured: require a valid session for everything except the login screen and the
      // password-recovery instructions (which a locked-out user reaches from it).
      if (!['/login', '/request-reset'].includes($page.url.pathname)) {
        const authed = await api.isAuthenticated();
        if (!authed) {
          await goto('/login');
          return;
        }
      }
      // Load which modules are installed (drives the switcher + dashboard visibility).
      ensureInstalled().catch(() => {});
      // The rail's Workspace section is the user's dashboard list.
      loadRail();
      // Adopt the persisted app-accent from the backend (source of truth across devices).
      settingsApi
        .getDefaults()
        .then((d) => {
          accent.hydrate(d.accent);
          tz.hydrate(d.default_timezone);
        })
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

<!-- Chart hover readout: any chart mark pushes its values here (see `ui/tip.svelte.js`). -->
<TipHost />

<!-- Step-up re-authentication. Mounted once here so any API call refused with
     `reauth_required` can raise the password prompt, wherever in the app it was made. -->
<ReauthModal />

<!-- Rail arrangement: which entries a section shows, and in which order. -->
<RailEditor
  bind:open={wsEditOpen}
  title={$t('rail.arrangeWorkspace')}
  items={workspaceCandidates}
  selected={workspaceEntries.map((entry) => entry.id)}
  max={WORKSPACE_MAX}
  onsave={(ids) => saveRail('workspace', ids)}
/>
<RailEditor
  bind:open={toolsEditOpen}
  title={$t('rail.arrangeTools')}
  items={toolCandidates}
  selected={toolEntries.map((entry) => entry.id)}
  max={toolCandidates.length}
  onsave={(ids) => saveRail('tools', ids)}
/>

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
  <div class="app" class:with-demo={demoOn} class:rail-closed={!railOpen}>
    <!-- Compact, grouped navigation mirrors the approved shell without inventing routes. -->
    {#if railOpen}
      <aside class="sidebar">
        <a class="brand" href="/" aria-label="OpenTraderWorld">
          <!-- Both marks ship; CSS below reveals the one that reads on the active theme. -->
          <img class="brand-mark on-light" src="/logo-dark.svg" alt="" aria-hidden="true" />
          <img class="brand-mark on-dark" src="/logo-light.svg" alt="" aria-hidden="true" />
          <span class="brand-copy"><strong>OTW</strong><small>OpenTraderWorld</small></span>
        </a>

        <nav class="sidebar-nav" aria-label="Workspace navigation">
          <div class="sidebar-group workspace">
            <div class="sidebar-group-head">
              <span class="sidebar-group-label">{$t('shell.workspace')}</span>
              <!-- Arranging the section is a rare act: the pencil only appears on hover. -->
              <button
                class="group-edit"
                title={$t('rail.arrange')}
                aria-label={$t('rail.arrange')}
                disabled={railBusy}
                onclick={() => (wsEditOpen = true)}
              >
                <Icon name="pencil" size={12} />
              </button>
            </div>
            <div class="sidebar-scroll">
              {#each workspaceEntries as entry (entry.id)}
                <a
                  class="sidebar-link"
                  class:active={selectedDashboard === entry.id}
                  href={dashboardHref(entry.id)}
                >
                  <Icon name={entry.icon} size={16} strokeWidth={1.8} />
                  <span>{entry.name}</span>
                </a>
              {/each}
            </div>
            <a class="sidebar-link sidebar-custom" href="/?new-dashboard=1">
              <Icon name="plus" size={16} strokeWidth={1.8} />
              <span>Custom +</span>
            </a>
          </div>
          {#if toolEntries.length}
            <div class="sidebar-group tools">
              <div class="sidebar-group-head">
                <span class="sidebar-group-label">{$t('shell.tools')}</span>
                <button
                  class="group-edit"
                  title={$t('rail.arrange')}
                  aria-label={$t('rail.arrange')}
                  disabled={railBusy}
                  onclick={() => (toolsEditOpen = true)}
                >
                  <Icon name="pencil" size={12} />
                </button>
              </div>
              <div class="sidebar-scroll">
                {#each toolEntries as mod (mod.id)}
                  <a
                    class="sidebar-link"
                    class:active={moduleForPath($page.url.pathname).id === mod.id}
                    href={mod.base}
                    aria-current={moduleForPath($page.url.pathname).id === mod.id ? 'page' : undefined}
                  >
                    <Icon name={mod.icon} size={16} strokeWidth={1.8} />
                    <span>{mod.name}</span>
                  </a>
                {/each}
              </div>
            </div>
          {/if}
        </nav>

      </aside>
    {/if}

    <div class="shell-main">
      <!-- Global top bar: module switcher, centered command access, then personal controls. -->
      <header class="topbar">
        <div class="switch-slot">
          <button
            class="cog rail-toggle"
            title={railOpen ? $t('shell.collapseNav') : $t('shell.expandNav')}
            aria-label={railOpen ? $t('shell.collapseNav') : $t('shell.expandNav')}
            aria-expanded={railOpen}
            onclick={toggleRail}
          >
            <Icon name="panel-left" size={17} />
          </button>
          <ModuleSwitcher />
        </div>
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
        <div class="topbar-actions">
          <button
            class="cog"
            class:active={privacy.hidden}
            title={privacy.hidden ? $t('shell.showData') : $t('shell.hideData')}
            aria-label={privacy.hidden ? $t('shell.showData') : $t('shell.hideData')}
            aria-pressed={privacy.hidden}
            onclick={togglePrivacy}
          >
            <Icon name={privacy.hidden ? 'eye-off' : 'eye'} size={17} />
          </button>
          <ThemeToggle />
          <a
            class="cog"
            class:active={$page.url.pathname.startsWith('/settings')}
            href="/settings"
            title={$t('nav.settings')}
            aria-label={$t('nav.settings')}
          >
            <Icon name="settings" size={17} />
          </a>
          <NotifBell />
          <ServiceStatus />
        </div>
      </header>

      <!-- Module context: each module renders its own sidebar + content here. -->
      <div class="module-context">
        {@render children?.()}
      </div>
    </div>
  </div>

  <!-- Slide-in reminder toasts (global, above all modules). -->
  <ToastBandeau />

  <!-- Floating assistant, on every page except /agent (the full chat lives there). -->
  {#if !$page.url.pathname.startsWith('/agent')}
    <AssistantWidget />
  {/if}

  <!-- Back to top, stacked over the assistant button when that one is there. -->
  <ScrollTop stacked={!$page.url.pathname.startsWith('/agent')} />
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
    grid-template-columns: var(--rail-width) minmax(0, 1fr);
    height: 100vh;
    background: var(--bg);
  }
  .app.with-demo {
    height: calc(100vh - 28px);
  }
  /* Collapsed: the rail column is gone entirely and the top bar runs edge to edge. */
  .app.rail-closed {
    grid-template-columns: minmax(0, 1fr);
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
    background: var(--surface-raised);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-3);
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
    color: var(--accent-contrast);
    border: none;
    border-radius: var(--radius-sm);
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
    display: grid;
    grid-template-columns: minmax(180px, 1fr) minmax(360px, 460px) minmax(180px, 1fr);
    align-items: center;
    gap: var(--space-3);
    /* The reference uses a slim, quiet workbar; module context belongs on its left. */
    padding-inline: 22px;
    /* Explicit stacking context above the module content: backdrop-filter alone creates
       one, but without a z-index Safari paints the bar's dropdowns behind positioned
       elements in the page (e.g. dashboard cards). */
    position: relative;
    z-index: var(--z-sticky);
    border-bottom: var(--hairline) solid var(--border);
    background: var(--shell-bg);
    backdrop-filter: blur(14px);
  }
  .shell-main {
    display: grid;
    grid-template-rows: var(--topbar-height) minmax(0, 1fr);
    min-width: 0;
    min-height: 0;
  }
  .sidebar {
    display: flex;
    flex-direction: column;
    min-height: 0;
    padding: 0;
    background: var(--sidebar-bg);
    border-right: var(--hairline) solid var(--border);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 9px;
    height: var(--topbar-height);
    padding: 0 18px;
    margin: 0;
    border-bottom: var(--hairline) solid var(--border);
    color: var(--text);
    text-decoration: none;
  }
  .brand-mark {
    display: block;
    width: 28px;
    height: 28px;
    object-fit: contain;
  }
  /* Light theme (explicit, or system default) shows the dark-ring mark; dark theme the
     light-ring one. Both branches are stated so the explicit choice always wins. */
  .brand-mark.on-dark {
    display: none;
  }
  :root[data-theme='dark'] .brand-mark.on-light {
    display: none;
  }
  :root[data-theme='dark'] .brand-mark.on-dark {
    display: block;
  }
  @media (prefers-color-scheme: dark) {
    :root:not([data-theme]) .brand-mark.on-light {
      display: none;
    }
    :root:not([data-theme]) .brand-mark.on-dark {
      display: block;
    }
  }
  .brand-copy {
    display: flex;
    flex-direction: column;
    line-height: 1;
  }
  .brand-copy strong {
    font-size: 15px;
    font-weight: var(--fw-medium);
    letter-spacing: 0.01em;
  }
  .brand-copy small {
    margin-top: 4px;
    color: var(--dim);
    font-size: 8px;
    letter-spacing: 0.02em;
  }
  .sidebar-nav {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow-y: auto;
    padding: 18px 10px;
    min-height: 0;
  }
  .sidebar-group + .sidebar-group {
    margin-top: 28px;
  }
  .sidebar-group-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding-right: 4px;
  }
  /* Arranging a section is rare: the pencil stays out of the way until the section is
     hovered or the button itself takes focus. */
  .group-edit {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    margin-bottom: 8px;
    padding: 0;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--dim);
    opacity: 0;
    cursor: pointer;
    transition: opacity var(--dur-fast) var(--ease), color var(--dur-fast) var(--ease);
  }
  .sidebar-group:hover .group-edit,
  .group-edit:focus-visible {
    opacity: 1;
  }
  .group-edit:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .group-edit:disabled {
    opacity: 0;
    cursor: not-allowed;
  }
  /* Workspace shows its eight entries and Custom + whole, never shrinking; Tools takes
     whatever height is left and scrolls past it. */
  .sidebar-scroll {
    overflow-y: auto;
    overscroll-behavior: contain;
    min-height: 0;
  }
  .sidebar-group {
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .sidebar-group.workspace {
    flex: 0 0 auto;
  }
  .sidebar-group.workspace .sidebar-scroll {
    overflow: visible;
  }
  .sidebar-group.tools {
    flex: 1 1 auto;
  }
  .sidebar-group-label {
    display: block;
    margin: 0 7px 8px;
    color: var(--dim);
    font-size: 10px;
    font-weight: var(--fw-medium);
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .sidebar-link {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 38px;
    padding: 0 9px;
    border: var(--hairline) solid transparent;
    border-radius: var(--radius);
    color: var(--muted);
    font-size: var(--fs-body);
    font-weight: var(--fw-medium);
    text-decoration: none;
    transition: background-color var(--dur-fast) var(--ease), color var(--dur-fast) var(--ease);
  }
  .sidebar-link :global(svg) {
    flex: 0 0 auto;
  }
  .sidebar-link:hover {
    background: var(--surface-2);
    color: var(--text);
  }
  .sidebar-link.active {
    background: color-mix(in srgb, var(--accent) 10%, var(--surface));
    color: var(--text);
  }
  .sidebar-link.active :global(svg) {
    color: var(--accent);
  }
  .switch-slot {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: min(280px, 100%);
  }
  /* Sits hard against the bar's left edge: it replaces the rail, it doesn't sit inside it. */
  .rail-toggle {
    flex: 0 0 auto;
    margin-left: -8px;
  }
  /* Search and Agent share the true center column of the global bar. */
  .search-slot {
    min-width: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
  }
  .search-slot :global(.gsearch) {
    flex: 1;
    max-width: 420px;
  }
  .topbar-actions {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--space-2);
  }

  /* Utility controls use a quiet square treatment; accent only identifies the active
     destination, never a whole header region. */
  .cog {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    height: 32px;
    border: var(--hairline) solid transparent;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--muted);
    font-family: inherit;
    font-size: var(--fs-body);
    cursor: pointer;
    text-decoration: none;
  }
  .cog {
    width: 32px;
    line-height: 1;
  }
  .cog:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .cog.active {
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 36%, var(--border-control));
    background: color-mix(in srgb, var(--accent) 10%, var(--surface));
  }
  /* Module context fills the area below the top bar; modules manage their own layout. */
  .module-context {
    min-height: 0;
    overflow: hidden;
    background: var(--bg);
  }

  /* Tablet: the bar runs out of room before the page does. Let the switcher
     shrink, then drop the dashboard label (its icon still says "grid"). The
     status indicator goes last — it's ambient, not actionable. */
  @media (max-width: 900px) {
    .app {
      grid-template-columns: 1fr;
    }
    .sidebar {
      display: none;
    }
    .switch-slot {
      width: auto;
      min-width: 0;
    }
    .rail-toggle {
      display: none;
    }
    .topbar {
      display: flex;
      gap: var(--space-2);
    }
    .search-slot {
      flex: 1;
    }
    .topbar-actions :global(.status) {
      display: none;
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
