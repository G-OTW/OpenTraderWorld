<script>
  // OAuth consent for MCP clients. The client sends the browser here with its
  // authorization request; the owner (signed in, the root layout made sure of it) picks
  // what the client may reach, and the answer goes back to the client's redirect URI.
  // Approving mints a credential, so it asks for the password like minting a token does.
  import '$lib/ui/auth-card.css';
  import { onMount } from 'svelte';
  import { t } from '$lib/i18n';
  import { settingsApi } from '$lib/settings/api.js';
  import { api } from '$lib/api';
  import { goto } from '$app/navigation';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  let params = $state({});
  let info = $state(null);
  let modules = $state([]);
  let name = $state('');
  let perms = $state({});
  let expiry = $state('0');
  let error = $state('');
  let fatal = $state('');
  let busy = $state(false);

  onMount(async () => {
    params = Object.fromEntries(new URLSearchParams(window.location.search));
    // Signed out: go through login and come back here with the request intact. Checked
    // before any API call, whose 401 would send the browser to login without the way back.
    if (!(await api.isAuthenticated())) {
      const here = window.location.pathname + window.location.search;
      await goto(`/login?next=${encodeURIComponent(here)}`);
      return;
    }
    try {
      info = await settingsApi.mcpOauthRequest(params);
      // A malformed request the client must hear about goes straight back to it.
      if (info.error_redirect) {
        window.location.href = info.error_redirect;
        return;
      }
      const s = await settingsApi.mcpSettings();
      modules = s.modules ?? [];
      name = info.client_name;
    } catch (e) {
      fatal = e.message;
    }
  });

  function setLevel(id, level) {
    const next = { ...perms };
    if (level) next[id] = level;
    else delete next[id];
    perms = next;
  }

  function setAll(level) {
    perms = level ? Object.fromEntries(modules.map((m) => [m.id, level])) : {};
  }

  async function approve() {
    if (!Object.keys(perms).length) {
      error = $t('oauth.needModule');
      return;
    }
    error = '';
    busy = true;
    try {
      const r = await settingsApi.mcpOauthApprove({
        ...params,
        name,
        permissions: perms,
        expires_in_days: Number(expiry)
      });
      window.location.href = r.redirect;
    } catch (e) {
      error = e.message;
      busy = false;
    }
  }

  async function deny() {
    busy = true;
    try {
      const r = await settingsApi.mcpOauthDeny(params);
      window.location.href = r.redirect;
    } catch (e) {
      error = e.message;
      busy = false;
    }
  }
</script>

<div class="auth-card consent">
  <h1>{$t('oauth.title')}</h1>
  {#if fatal}
    <p class="sub">{$t('oauth.invalid')}</p>
    <ErrorText error={fatal} compact copyable />
  {:else if !info}
    <p class="sub">…</p>
  {:else}
    <p class="sub">
      {$t('oauth.lead.before')}<strong>{info.client_name}</strong>{$t('oauth.lead.after')}
      {$t('oauth.redirect')} <strong>{info.redirect_host}</strong>.
    </p>
    <p class="warn">{$t('oauth.warn')}</p>

    <label for="cname">{$t('oauth.name')}</label>
    <input id="cname" bind:value={name} maxlength="80" />

    <label for="cexp">{$t('settings.mcp.expiry')}</label>
    <Dropdown
      value={expiry}
      onpick={(v) => (expiry = v)}
      ariaLabel={$t('settings.mcp.expiry')}
      options={[
        { value: '0', label: $t('settings.mcp.expiryNever') },
        { value: '30', label: $t('settings.mcp.expiryDays', { n: 30 }) },
        { value: '90', label: $t('settings.mcp.expiryDays', { n: 90 }) },
        { value: '365', label: $t('settings.mcp.expiryDays', { n: 365 }) }
      ]}
    />

    <div class="permhead">
      <span>{$t('settings.mcp.permissions')}</span>
      <span class="bulk">
        <button class="ghost" onclick={() => setAll('r')}>{$t('settings.mcp.allRead')}</button>
        <button class="ghost" onclick={() => setAll('rw')}>{$t('settings.mcp.allRw')}</button>
        <button class="ghost" onclick={() => setAll('')}>{$t('common.clear')}</button>
      </span>
    </div>
    <div class="matrix">
      {#each modules as m (m.id)}
        <div class="permrow">
          <span>{m.label}</span>
          <Dropdown
            value={perms[m.id] ?? ''}
            onpick={(v) => setLevel(m.id, v)}
            ariaLabel={m.label}
            options={[
              { value: '', label: $t('settings.mcp.levelNone') },
              { value: 'r', label: $t('settings.mcp.levelRead') },
              { value: 'rw', label: $t('settings.mcp.levelRw') },
              { value: 'rwd', label: $t('settings.mcp.levelFull') }
            ]}
          />
        </div>
      {/each}
    </div>

    <ErrorText error={error} compact copyable />

    <button class="primary" disabled={busy} onclick={approve}>
      {busy ? $t('oauth.working') : $t('oauth.approve')}
    </button>
    <div class="alt">
      <button class="ghost" disabled={busy} onclick={deny}>{$t('oauth.deny')}</button>
    </div>
  {/if}
</div>

<style>
  .consent {
    max-width: 520px;
  }
  .warn {
    border-left: 3px solid var(--amber);
    background: var(--surface-2);
    padding: var(--space-2) var(--space-3);
    font-size: var(--text-sm);
    color: var(--muted);
    margin: 0 0 var(--space-2);
  }
  .consent :global(.dd) {
    width: 100%;
  }
  .permhead {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-top: var(--space-4);
    margin-bottom: var(--space-1);
    color: var(--dim);
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
  }
  .bulk {
    display: inline-flex;
    gap: var(--space-2);
  }
  .matrix {
    border: var(--hairline) solid var(--border);
    max-height: 280px;
    overflow-y: auto;
  }
  .permrow {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    padding: 6px 10px;
    border-bottom: var(--hairline) solid var(--border);
    font-size: var(--text-sm);
  }
  .permrow:last-child {
    border-bottom: none;
  }
  .permrow :global(.dd) {
    width: 130px;
    flex: none;
  }
  button.ghost {
    background: transparent;
    border: var(--hairline) solid var(--border);
    color: var(--muted);
    border-radius: var(--radius);
    padding: 3px 8px;
    font-size: var(--text-xs);
    cursor: pointer;
  }
  button.ghost:hover:not(:disabled) {
    color: var(--text);
  }
  @media (max-width: 480px) {
    .consent { width: calc(100% - 2 * var(--space-3)); min-width: 0; padding: var(--space-4); }
    .consent strong { overflow-wrap: anywhere; }
    .permhead { display: block; }
    .bulk { display: flex; flex-wrap: wrap; margin-top: var(--space-2); }
    .bulk button, .alt button, .consent > button.primary { min-height: 44px; }
    .permrow { min-width: 0; }
    .permrow > span { min-width: 0; overflow-wrap: anywhere; }
    .permrow :global(.dd) { width: min(130px, 48%); }
  }
</style>
