<script>
  // Settings page. A left rail selects a section; the content area renders it. Sections
  // are independent and load their own data. Heavy host actions (backup, update) are
  // guided rather than executed by the app.
  import AccountSection from '$lib/settings/AccountSection.svelte';
  import DefaultsSection from '$lib/settings/DefaultsSection.svelte';
  import AppearanceSection from '$lib/settings/AppearanceSection.svelte';
  import NetworkSection from '$lib/settings/NetworkSection.svelte';
  import ModulesSection from '$lib/settings/ModulesSection.svelte';
  import DataSection from '$lib/settings/DataSection.svelte';
  import BackupSection from '$lib/settings/BackupSection.svelte';
  import UpdateSection from '$lib/settings/UpdateSection.svelte';
  import LogsSection from '$lib/settings/LogsSection.svelte';
  import RateSection from '$lib/settings/RateSection.svelte';
  import McpSection from '$lib/settings/McpSection.svelte';
  import CreditsSection from '$lib/settings/CreditsSection.svelte';
  import AboutSection from '$lib/settings/AboutSection.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';

  // `labelKey` resolves through $t at render time so the rail relabels on language change.
  const sections = [
    { id: 'account', labelKey: 'settings.nav.account', icon: 'user' },
    { id: 'defaults', labelKey: 'settings.nav.defaults', icon: 'settings' },
    { id: 'appearance', labelKey: 'settings.nav.appearance', icon: 'droplet' },
    { id: 'network', labelKey: 'settings.nav.network', icon: 'globe' },
    { id: 'modules', labelKey: 'settings.nav.modules', icon: 'grid' },
    { id: 'data', labelKey: 'settings.nav.data', icon: 'database' },
    { id: 'backup', labelKey: 'settings.nav.backup', icon: 'save' },
    { id: 'update', labelKey: 'settings.nav.update', icon: 'refresh-cw' },
    { id: 'logs', labelKey: 'settings.nav.logs', icon: 'file-text' },
    { id: 'rate', labelKey: 'settings.nav.rate', icon: 'bar-chart' },
    { id: 'mcp', labelKey: 'settings.nav.mcp', icon: 'zap' },
    { id: 'credits', labelKey: 'settings.nav.credits', icon: 'book-open' },
    { id: 'about', labelKey: 'settings.nav.about', icon: 'info' }
  ];

  // Persist the open section in the URL hash so it survives a reload and is shareable.
  const validId = (id) => (sections.some((s) => s.id === id) ? id : null);
  const hashId = () =>
    typeof location !== 'undefined' ? validId(location.hash.replace('#', '')) : null;

  let active = $state(hashId() ?? 'account');

  function select(id) {
    active = id;
    if (typeof location !== 'undefined') location.hash = id;
  }

  // Reflect back/forward navigation and manual hash edits.
  $effect(() => {
    const onHash = () => {
      const id = hashId();
      if (id) active = id;
    };
    window.addEventListener('hashchange', onHash);
    return () => window.removeEventListener('hashchange', onHash);
  });
</script>

<div class="page">
  <aside class="rail">
    <h1>{$t('settings.title')}</h1>
    <nav>
      {#each sections as s (s.id)}
        <button class="nav" class:active={active === s.id} onclick={() => select(s.id)}>
          <span class="ico"><Icon name={s.icon} size={15} /></span>{$t(s.labelKey)}
        </button>
      {/each}
    </nav>
  </aside>

  <section class="content">
    {#if active === 'account'}
      <AccountSection />
    {:else if active === 'defaults'}
      <DefaultsSection />
    {:else if active === 'appearance'}
      <AppearanceSection />
    {:else if active === 'network'}
      <NetworkSection />
    {:else if active === 'modules'}
      <ModulesSection />
    {:else if active === 'data'}
      <DataSection />
    {:else if active === 'backup'}
      <BackupSection />
    {:else if active === 'update'}
      <UpdateSection />
    {:else if active === 'logs'}
      <LogsSection />
    {:else if active === 'rate'}
      <RateSection />
    {:else if active === 'mcp'}
      <McpSection />
    {:else if active === 'credits'}
      <CreditsSection />
    {:else if active === 'about'}
      <AboutSection />
    {/if}
  </section>
</div>

<style>
  .page {
    display: flex;
    height: 100%;
    min-height: 0;
  }
  .rail {
    width: 220px;
    flex-shrink: 0;
    border-right: 1px solid var(--border);
    padding: var(--space-4);
    background: var(--surface);
    overflow-y: auto;
  }
  .rail h1 {
    margin: 0 0 var(--space-4);
    font-size: var(--text-md);
    color: var(--text);
  }
  nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .nav {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: transparent;
    border: none;
    color: var(--text);
    padding: 9px 10px;
    border-radius: var(--radius);
    cursor: pointer;
    font-size: var(--text-base);
    text-align: left;
  }
  .nav:hover {
    background: var(--surface-2);
  }
  .nav.active {
    background: var(--surface-2);
    color: var(--accent);
  }
  .ico {
    width: 1.2em;
    display: inline-flex;
    justify-content: center;
    color: var(--muted);
  }
  .nav.active .ico {
    color: var(--accent);
  }
  .content {
    flex: 1;
    min-width: 0;
    padding: var(--space-6);
    overflow-y: auto;
  }
</style>
