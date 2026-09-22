<script>
  // Settings page. A left rail selects a section; the content area renders it. Sections
  // are independent and load their own data. Heavy host actions (backup, update) are
  // guided rather than executed by the app.
  import AccountSection from '$lib/settings/AccountSection.svelte';
  import SecuritySection from '$lib/settings/SecuritySection.svelte';
  import DefaultsSection from '$lib/settings/DefaultsSection.svelte';
  import AppearanceSection from '$lib/settings/AppearanceSection.svelte';
  import NetworkSection from '$lib/settings/NetworkSection.svelte';
  import ModulesSection from '$lib/settings/ModulesSection.svelte';
  import DataSection from '$lib/settings/DataSection.svelte';
  import BackupSection from '$lib/settings/BackupSection.svelte';
  import UpdateSection from '$lib/settings/UpdateSection.svelte';
  import LogsSection from '$lib/settings/LogsSection.svelte';
  import RateSection from '$lib/settings/RateSection.svelte';
  import ConnectorsSection from '$lib/settings/ConnectorsSection.svelte';
  import BrokersSection from '$lib/settings/BrokersSection.svelte';
  import NotificationsSection from '$lib/settings/NotificationsSection.svelte';
  import VaultSection from '$lib/settings/VaultSection.svelte';
  import McpSection from '$lib/settings/McpSection.svelte';
  import ControlSection from '$lib/settings/ControlSection.svelte';
  import CreditsSection from '$lib/settings/CreditsSection.svelte';
  import AboutSection from '$lib/settings/AboutSection.svelte';
  import TermsSection from '$lib/settings/TermsSection.svelte';
  import LicenseSection from '$lib/settings/LicenseSection.svelte';
  import RiskDisclosureSection from '$lib/settings/RiskDisclosureSection.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';

  // `labelKey` resolves through $t at render time so the rail relabels on language change.
  // The rail is grouped: each group is a titled block, and `sections` stays the flat list
  // the hash lookup and the content switch read from.
  const groups = [
    {
      labelKey: 'settings.group.general',
      items: [
        { id: 'account', labelKey: 'settings.nav.account', icon: 'user' },
        { id: 'appearance', labelKey: 'settings.nav.appearance', icon: 'droplet' },
        { id: 'defaults', labelKey: 'settings.nav.defaults', icon: 'settings' },
        { id: 'modules', labelKey: 'settings.nav.modules', icon: 'grid' }
      ]
    },
    {
      labelKey: 'settings.group.access',
      items: [
        { id: 'security', labelKey: 'settings.nav.security', icon: 'shield' },
        { id: 'network', labelKey: 'settings.nav.network', icon: 'globe' },
        { id: 'vault', labelKey: 'settings.nav.vault', icon: 'lock' }
      ]
    },
    {
      labelKey: 'settings.group.connections',
      items: [
        { id: 'connectors', labelKey: 'settings.nav.connectors', icon: 'plug' },
        { id: 'brokers', labelKey: 'settings.nav.brokers', icon: 'briefcase' },
        { id: 'notifications', labelKey: 'settings.nav.notifications', icon: 'bell' },
        { id: 'mcp', labelKey: 'settings.nav.mcp', icon: 'zap' },
        { id: 'control', labelKey: 'settings.nav.control', icon: 'message-square' }
      ]
    },
    {
      labelKey: 'settings.group.data',
      items: [
        { id: 'data', labelKey: 'settings.nav.data', icon: 'database' },
        { id: 'backup', labelKey: 'settings.nav.backup', icon: 'save' }
      ]
    },
    {
      labelKey: 'settings.group.system',
      items: [
        { id: 'update', labelKey: 'settings.nav.update', icon: 'refresh-cw' },
        { id: 'logs', labelKey: 'settings.nav.logs', icon: 'file-text' },
        { id: 'rate', labelKey: 'settings.nav.rate', icon: 'bar-chart' },
        { id: 'credits', labelKey: 'settings.nav.credits', icon: 'book-open' },
        { id: 'about', labelKey: 'settings.nav.about', icon: 'info' }
      ]
    },
    {
      labelKey: 'settings.group.legal',
      items: [
        { id: 'terms', labelKey: 'settings.nav.terms', icon: 'file-text' },
        { id: 'license', labelKey: 'settings.nav.license', icon: 'book-open' },
        { id: 'risks', labelKey: 'settings.nav.risks', icon: 'alert-triangle' }
      ]
    }
  ];

  const sections = groups.flatMap((g) => g.items);

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
      {#each groups as g (g.labelKey)}
        <p class="group">{$t(g.labelKey)}</p>
        {#each g.items as s (s.id)}
          <button class="nav" class:active={active === s.id} onclick={() => select(s.id)}>
            <span class="ico"><Icon name={s.icon} size={15} /></span>{$t(s.labelKey)}
          </button>
        {/each}
      {/each}
    </nav>
  </aside>

  <section class="content">
    {#if active === 'account'}
      <AccountSection />
    {:else if active === 'security'}
      <SecuritySection />
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
    {:else if active === 'connectors'}
      <ConnectorsSection />
    {:else if active === 'brokers'}
      <BrokersSection />
    {:else if active === 'notifications'}
      <NotificationsSection />
    {:else if active === 'vault'}
      <VaultSection />
    {:else if active === 'mcp'}
      <McpSection />
    {:else if active === 'control'}
      <ControlSection />
    {:else if active === 'credits'}
      <CreditsSection />
    {:else if active === 'about'}
      <AboutSection />
    {:else if active === 'terms'}
      <TermsSection />
    {:else if active === 'license'}
      <LicenseSection />
    {:else if active === 'risks'}
      <RiskDisclosureSection />
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
    border-right: var(--hairline) solid var(--border);
    padding: var(--space-4);
    background: var(--surface);
    overflow-y: auto;
  }
  .rail h1 {
    margin: 0 0 var(--space-4);
    font-size: var(--text-lg);
    font-weight: var(--fw-medium);
    letter-spacing: 0.01em;
    color: var(--text);
  }
  nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .group {
    margin: var(--space-4) 0 var(--space-1);
    padding: 0 var(--space-2) 0 14px;
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--dim);
  }
  .group:first-child {
    margin-top: 0;
  }
  .nav {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: transparent;
    border: none;
    border-left: 1.5px solid transparent;
    color: var(--muted);
    padding: 8px 16px 8px 14px;
    border-radius: 0;
    cursor: pointer;
    font-size: var(--text-base);
    text-align: left;
  }
  .nav:hover {
    background: var(--surface-2);
    color: var(--text);
  }
  .nav.active {
    background: var(--surface-2);
    border-left-color: var(--accent);
    color: var(--text);
  }
  .ico {
    width: 1.2em;
    display: inline-flex;
    justify-content: center;
    color: var(--dim);
  }
  .nav:hover .ico,
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
