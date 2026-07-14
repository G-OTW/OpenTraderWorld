<script>
  // Network exposure: how the app binds to the host. Secure by default (localhost only);
  // LAN and Web are opt-in. Core can't rebind a live socket, so saving writes a secret-free
  // network.env and the operator runs `docker compose up -d` to apply (banner shown below).
  import { onMount } from 'svelte';
  import { settingsApi } from '$lib/settings/api.js';
  import CommandBlock from '$lib/settings/CommandBlock.svelte';
  import { t } from '$lib/i18n';

  const RESTART_CMD =
    'docker compose -f deploy/docker-compose.yml --env-file deploy/.env --env-file deploy/network.env up -d';

  // Labels/hints resolved via $t at render (see template) so they relabel on language change.
  const MODES = [
    { id: 'local', labelKey: 'settings.network.mode.local', hintKey: 'settings.network.mode.localHint' },
    { id: 'lan', labelKey: 'settings.network.mode.lan', hintKey: 'settings.network.mode.lanHint' },
    { id: 'lan_https', labelKey: 'settings.network.mode.lanhttps', hintKey: 'settings.network.mode.lanhttpsHint' },
    { id: 'web', labelKey: 'settings.network.mode.web', hintKey: 'settings.network.mode.webHint' }
  ];

  let mode = $state('local');
  let port = $state(5454);
  let domain = $state('');
  let dnsProvider = $state('duckdns');
  let dnsToken = $state('');
  let dnsTokenSet = $state(false); // a token is already saved server-side
  let lanIp = $state('');

  let loading = $state(true);
  let saving = $state(false);
  let error = $state('');
  let restart = $state(false); // show the "restart required" banner after a successful save

  onMount(async () => {
    try {
      const n = await settingsApi.getNetwork();
      mode = n.mode ?? 'local';
      port = Number(n.port) || 5454;
      domain = n.domain ?? '';
      dnsProvider = n.dns_provider || 'duckdns';
      dnsTokenSet = !!n.dns_token_set;
      lanIp = n.lan_ip ?? '';
    } finally {
      loading = false;
    }
  });

  // Best-guess LAN IP: when the app is already opened via a private IPv4, that IS the
  // host's LAN address. Applied once as a prefill; the user can always override.
  $effect(() => {
    if (mode === 'lan_https' && !lanIp) {
      const h = window.location.hostname;
      if (/^(10\.|192\.168\.|172\.(1[6-9]|2\d|3[01])\.)\d+\.\d+$/.test(h)) lanIp = h;
    }
  });

  async function save() {
    error = '';
    restart = false;
    if ((mode === 'web' || mode === 'lan_https') && !domain.trim()) {
      error = $t('settings.network.err.domainRequired');
      return;
    }
    if (mode === 'lan_https' && !dnsToken.trim() && !dnsTokenSet) {
      error = $t('settings.network.err.dnsTokenRequired');
      return;
    }
    if (mode === 'lan_https' && dnsProvider === 'duckdns' && !lanIp.trim()) {
      error = $t('settings.network.err.lanIpRequired');
      return;
    }
    saving = true;
    try {
      await settingsApi.setNetwork({
        mode,
        port: Number(port),
        domain: domain.trim(),
        dns_provider: dnsProvider,
        dns_token: dnsToken.trim(),
        lan_ip: lanIp.trim()
      });
      restart = true;
      if (dnsToken.trim()) dnsTokenSet = true;
      dnsToken = '';
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }
</script>

<div class="section">
  <h2>{$t('settings.network.title')}</h2>
  <p class="muted small">{$t('settings.network.subtitle')}</p>

  {#if loading}
    <p class="muted">{$t('common.loading')}</p>
  {:else}
    <form
      class="form"
      onsubmit={(e) => {
        e.preventDefault();
        save();
      }}
    >
      <fieldset class="modes">
        {#each MODES as m}
          <label class="mode" class:sel={mode === m.id}>
            <input type="radio" name="mode" value={m.id} bind:group={mode} />
            <span class="mode-body">
              <span class="mode-label">{$t(m.labelKey)}</span>
              <span class="mode-hint">{$t(m.hintKey)}</span>
            </span>
          </label>
        {/each}
      </fieldset>

      {#if mode === 'local' || mode === 'lan'}
        <label class="field">
          <span>{$t('settings.network.port')}</span>
          <input type="number" min="1" max="65535" bind:value={port} />
        </label>
      {:else if mode === 'web'}
        <label class="field">
          <span>{$t('settings.network.domain')}</span>
          <input type="text" placeholder="app.example.com" bind:value={domain} />
          <small class="muted">{$t('settings.network.domainHint')}</small>
        </label>
      {:else}
        <p class="muted small">{$t('settings.network.lanHttpsHelp')}</p>
        <label class="field">
          <span>{$t('settings.network.dnsProvider')}</span>
          <select bind:value={dnsProvider}>
            <option value="duckdns">DuckDNS (free)</option>
            <option value="cloudflare">Cloudflare</option>
          </select>
        </label>
        <label class="field">
          <span>{$t('settings.network.domain')}</span>
          <input
            type="text"
            placeholder={dnsProvider === 'duckdns' ? 'yourname.duckdns.org' : 'otw.example.com'}
            bind:value={domain}
          />
          {#if dnsProvider === 'cloudflare'}
            <small class="muted">{$t('settings.network.cfDomainHint')}</small>
          {/if}
        </label>
        <label class="field">
          <span>{$t('settings.network.dnsToken')}</span>
          <input
            type="password"
            autocomplete="off"
            placeholder={dnsTokenSet ? $t('settings.network.dnsTokenKeep') : ''}
            bind:value={dnsToken}
          />
          <small class="muted">{$t('settings.network.dnsTokenHint')}</small>
        </label>
        {#if dnsProvider === 'duckdns'}
          <label class="field">
            <span>{$t('settings.network.lanIp')}</span>
            <input type="text" placeholder="192.168.1.20" bind:value={lanIp} />
            <small class="muted">{$t('settings.network.lanIpHint')}</small>
          </label>
        {/if}
        <p class="muted small">{$t('settings.network.ctNote')}</p>
      {/if}

      {#if error}<p class="err">{error}</p>{/if}

      <div class="actions">
        <button type="submit" class="primary" disabled={saving}>
          {saving ? $t('common.saving') : $t('common.save')}
        </button>
      </div>
    </form>

    {#if restart}
      <div class="restart">
        <strong>{$t('settings.network.restartTitle')}</strong> {$t('settings.network.restartBody')}
        <CommandBlock command={RESTART_CMD} />
        <p class="muted small">{$t('settings.network.restartNote')}</p>
      </div>
    {/if}
  {/if}
</div>

<style>
  .section {
    max-width: 520px;
  }
  h2 {
    margin: 0 0 var(--space-1);
    font-size: 1.1rem;
    color: var(--text);
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    margin-top: var(--space-4);
  }
  .modes {
    border: none;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .mode {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    padding: var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    cursor: pointer;
  }
  .mode.sel {
    border-color: var(--accent);
    background: var(--surface-2);
  }
  .mode input {
    margin-top: 3px;
  }
  .mode-body {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .mode-label {
    font-size: var(--text-base);
    color: var(--text);
  }
  .mode-hint {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
  }
  .restart {
    margin-top: var(--space-4);
    padding: var(--space-3);
    border: 1px solid var(--amber);
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--amber) 10%, transparent);
    font-size: var(--text-base);
    color: var(--text);
  }
  .muted {
    color: var(--muted);
  }
  .small {
    font-size: var(--text-xs);
  }
  .err {
    color: var(--red);
    font-size: var(--text-base);
    margin: 0;
  }
</style>
