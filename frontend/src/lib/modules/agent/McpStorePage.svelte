<script>
  // MCP store — full-page section of the Agent module. Two parts: "My servers" (add /
  // edit / test / delete remote Streamable-HTTP MCP servers; auth values are write-only
  // and sealed at rest) and a curated catalog of well-known servers whose entries
  // pre-fill the add form. Servers are then enabled per conversation from the chat
  // header's tools dropdown.
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { t } from '$lib/i18n';
  import Icon from '$lib/ui/Icon.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Input from '$lib/ui/Input.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import PageHeader from '$lib/ui/PageHeader.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import VaultPicker from '$lib/vault/VaultPicker.svelte';
  import { agentApi } from '$lib/modules/agent/api.js';
  import { settingsApi } from '$lib/settings/api.js';
  import { MCP_CATALOG } from '$lib/modules/agent/catalog.js';
  import ConnectOtwModal from '$lib/modules/agent/ConnectOtwModal.svelte';

  let connectOpen = $state(false);

  let servers = $state([]);
  let loading = $state(true);
  let error = $state('');

  // Built-in gateway state, for the self-hosted catalog card. Failure is non-fatal: the
  // card just shows as not-yet-enabled rather than breaking the page.
  let gatewayOn = $state(false);

  // null = closed, {} = new custom, {catalog} = new from catalog, row = edit.
  let editing = $state(null);
  let form = $state({ name: '', url: '', auth_header: 'Authorization', auth_value: '', auth_vault_item: null, catalog_id: '', enabled: true });
  let formErr = $state('');
  let saving = $state(false);

  // Per-server test state: id → { busy, ok, tools, error }.
  let tests = $state({});

  // Modal confirm (replaces native confirm()).
  let confirmOpen = $state(false);
  let confirmMessage = $state('');
  let onConfirmYes = $state(() => {});
  function askConfirm(message, onyes) {
    confirmMessage = message;
    onConfirmYes = onyes;
    confirmOpen = true;
  }

  onMount(reload);

  async function reload() {
    loading = true;
    error = '';
    try {
      servers = await agentApi.listMcpServers();
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
    try {
      gatewayOn = !!(await settingsApi.mcpSettings())?.enabled;
    } catch {
      gatewayOn = false;
    }
  }

  function openNew(entry = null) {
    editing = entry ? { catalog: entry } : {};
    form = entry
      ? {
          name: entry.name,
          url: entry.url,
          auth_header: entry.authHeader || 'Authorization',
          auth_value: '',
          auth_vault_item: null,
          catalog_id: entry.id,
          enabled: true
        }
      : { name: '', url: '', auth_header: 'Authorization', auth_value: '', auth_vault_item: null, catalog_id: '', enabled: true };
    formErr = '';
  }
  function openEdit(s) {
    editing = s;
    // auth_value stays blank on edit — empty means "leave unchanged". An existing vault
    // reference is pre-selected; switching to a pasted value just needs typing one.
    form = {
      name: s.name,
      url: s.url,
      auth_header: s.auth_header,
      auth_value: '',
      auth_vault_item: s.auth_vault_item ?? null,
      catalog_id: s.catalog_id,
      enabled: s.enabled
    };
    formErr = '';
  }

  async function save() {
    formErr = '';
    if (!form.name.trim()) {
      formErr = $t('agent.mcp.errName');
      return;
    }
    if (!/^https?:\/\//.test(form.url.trim())) {
      formErr = $t('agent.mcp.errUrl');
      return;
    }
    saving = true;
    try {
      if (editing.id) await agentApi.updateMcpServer(editing.id, form);
      else await agentApi.addMcpServer(form);
      editing = null;
      await reload();
    } catch (e) {
      formErr = e.message;
    } finally {
      saving = false;
    }
  }

  function remove(s) {
    askConfirm($t('agent.mcp.deleteConfirm', { name: s.name }), async () => {
      try {
        await agentApi.deleteMcpServer(s.id);
        await reload();
      } catch (e) {
        error = e.message;
      }
    });
  }

  async function toggle(s) {
    try {
      await agentApi.updateMcpServer(s.id, { ...s, auth_value: '' , enabled: !s.enabled });
      await reload();
    } catch (e) {
      error = e.message;
    }
  }

  /** Probe the server: connect + list tools; result shown inline on the row. */
  async function test(s) {
    tests = { ...tests, [s.id]: { busy: true } };
    try {
      const tools = await agentApi.testMcpServer(s.id);
      tests = { ...tests, [s.id]: { busy: false, ok: true, tools } };
    } catch (e) {
      tests = { ...tests, [s.id]: { busy: false, ok: false, error: e.message } };
    }
  }

  const inCatalog = $derived(new Set(servers.map((s) => s.catalog_id).filter(Boolean)));
</script>

<div class="store">
  <PageHeader title={$t('agent.mcp.title')} subtitle={$t('agent.mcp.subtitle')}>
    {#snippet actions()}
      <Button variant="ghost" icon="arrow-left" onclick={() => goto('/agent')}>{$t('common.back')}</Button>
      {#if !editing}
        <Button variant="primary" icon="plus" onclick={() => openNew()}>{$t('agent.mcp.addCustom')}</Button>
      {/if}
    {/snippet}
  </PageHeader>

  <p class="security"><Icon name="alert-triangle" size={14} /> {$t('agent.mcp.securityNote')}</p>

  <ErrorText {error} />

  {#if editing}
    <div class="form">
      <h3>{editing.id ? editing.name : editing.catalog ? editing.catalog.name : $t('agent.mcp.addCustom')}</h3>
      {#if editing.catalog?.authHint}
        <p class="muted">{editing.catalog.authHint}</p>
      {/if}
      <div class="grid2">
        <Input label={$t('agent.mcp.name')} bind:value={form.name} />
        <Input label={$t('agent.mcp.url')} placeholder="https://example.com/mcp" bind:value={form.url} />
        <Input label={$t('agent.mcp.authHeader')} placeholder="Authorization" bind:value={form.auth_header} />
        <div class="vfield">
          <span class="vlabel">{$t('agent.mcp.authValue')}</span>
          <VaultPicker
            bind:vaultItemId={form.auth_vault_item}
            hasSecret={!!editing.id && editing.has_auth}
          />
          <span class="vhint">{$t('agent.mcp.authHint')}</span>
        </div>
      </div>
      <label class="check"><input type="checkbox" bind:checked={form.enabled} /> {$t('agent.set.enabled')}</label>
      <ErrorText error={formErr} />
      <div class="row">
        <Button variant="primary" loading={saving} onclick={save}>{$t('common.save')}</Button>
        <Button variant="ghost" onclick={() => (editing = null)}>{$t('common.cancel')}</Button>
      </div>
    </div>
  {/if}

  <!-- My servers -->
  <section>
    <h2>{$t('agent.mcp.mine')}</h2>
    {#if loading}
      <p class="muted">{$t('common.loading')}</p>
    {:else if !servers.length}
      <EmptyState icon="link" title={$t('agent.mcp.empty')} description={$t('agent.mcp.emptyDesc')} />
    {:else}
      <ul class="list">
        {#each servers as s (s.id)}
          <li class:disabled={!s.enabled}>
            <div class="main">
              <div class="name-row">
                <strong>{s.name}</strong>
                {#if s.has_auth}<Badge tone="success">{$t('agent.mcp.authSet')}</Badge>{:else}<Badge tone="neutral">{$t('agent.mcp.noAuth')}</Badge>{/if}
                {#if s.catalog_id}<Badge tone="accent">{$t('agent.mcp.fromCatalog')}</Badge>{/if}
                {#if !s.enabled}<Badge tone="danger">{$t('agent.set.disabled')}</Badge>{/if}
              </div>
              <span class="url">{s.url}</span>
              {#if tests[s.id]?.busy}
                <span class="test muted">{$t('agent.mcp.testing')}</span>
              {:else if tests[s.id]?.ok}
                <span class="test ok">
                  <Icon name="check" size={12} />
                  {$t('agent.mcp.testOk', { count: tests[s.id].tools.length })}
                  <code>{tests[s.id].tools.slice(0, 6).join(', ')}{tests[s.id].tools.length > 6 ? '…' : ''}</code>
                </span>
              {:else if tests[s.id]}
                <span class="test err"><Icon name="alert-triangle" size={12} /> {tests[s.id].error}</span>
              {/if}
            </div>
            <div class="actions">
              <Button size="sm" variant="ghost" icon="zap" loading={tests[s.id]?.busy} onclick={() => test(s)}>{$t('agent.mcp.test')}</Button>
              <button class="icon" title={s.enabled ? $t('agent.skl.disable') : $t('agent.skl.enable')} onclick={() => toggle(s)}>
                <Icon name={s.enabled ? 'check-circle' : 'eye-off'} size={14} />
              </button>
              <button class="icon" title={$t('agent.edit')} onclick={() => openEdit(s)}><Icon name="pencil" size={14} /></button>
              <button class="icon danger" title={$t('agent.delete')} onclick={() => remove(s)}><Icon name="trash" size={14} /></button>
            </div>
          </li>
        {/each}
      </ul>
    {/if}
  </section>

  <!-- Catalog -->
  <section>
    <h2>{$t('agent.mcp.catalog')}</h2>
    <p class="muted">{$t('agent.mcp.catalogNote')}</p>
    <div class="cards">
      {#each MCP_CATALOG as entry (entry.id)}
        <div class="card" class:muted-card={entry.selfHosted && gatewayOn}>
          <div class="card-head">
            <strong>{entry.name}</strong>
            {#if entry.selfHosted}
              <Badge tone={gatewayOn ? 'success' : 'neutral'}>
                {gatewayOn ? $t('agent.mcp.builtinOn') : $t('agent.mcp.builtin')}
              </Badge>
            {:else if inCatalog.has(entry.id)}
              <Badge tone="success">{$t('agent.mcp.added')}</Badge>
            {/if}
          </div>
          <p class="desc">{entry.description}</p>
          <p class="auth-note">
            {#if entry.authHint}<Icon name="alert-triangle" size={11} /> {entry.authHint}{:else}<Icon name="check" size={11} /> {$t('agent.mcp.noAuthNeeded')}{/if}
          </p>
          <div class="card-actions">
            {#if entry.selfHosted}
              {#if gatewayOn}
                <span></span>
                <Button size="sm" variant="ghost" icon="settings" onclick={() => goto('/settings#mcp')}>
                  {$t('agent.mcp.manageBuiltin')}
                </Button>
              {:else}
                <a class="docs" href="/settings#mcp">
                  {$t('agent.mcp.manageBuiltin')} <Icon name="external-link" size={11} />
                </a>
                <Button size="sm" icon="zap" onclick={() => (connectOpen = true)}>
                  {$t('agent.mcp.connect.action')}
                </Button>
              {/if}
            {:else}
              <a class="docs" href={entry.docsUrl} target="_blank" rel="noopener noreferrer">
                {$t('agent.mcp.docs')} <Icon name="external-link" size={11} />
              </a>
              <Button size="sm" icon="plus" onclick={() => openNew(entry)} disabled={inCatalog.has(entry.id)}>
                {$t('agent.mcp.add')}
              </Button>
            {/if}
          </div>
        </div>
      {/each}
    </div>
  </section>
</div>

<ConnectOtwModal bind:open={connectOpen} ondone={reload} />

<ConfirmModal
  bind:open={confirmOpen}
  title={$t('agent.confirm.title')}
  message={confirmMessage}
  confirmLabel={$t('common.delete')}
  danger
  onconfirm={onConfirmYes}
/>

<style>
  .store {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    padding: var(--space-4);
    max-width: 980px;
    margin: 0 auto;
    width: 100%;
    overflow-y: auto;
  }
  .security {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    margin: 0;
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--amber);
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--amber) 10%, transparent);
    color: var(--text);
    font-size: var(--text-sm);
  }
  section {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  h2 {
    margin: 0;
    font-size: 1.05rem;
    color: var(--text);
  }
  h3 {
    margin: 0;
    font-size: 1rem;
    color: var(--text);
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    border: 1px solid var(--border-control);
    border-radius: var(--radius-lg, var(--radius));
    padding: var(--space-4);
    background: var(--surface);
  }
  .grid2 {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-2) var(--space-3);
  }
  .vfield {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .vlabel,
  .vhint {
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .row {
    display: flex;
    gap: var(--space-2);
  }
  .check {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .list {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
  }
  .list li {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    background: var(--surface);
  }
  .list li.disabled {
    opacity: 0.65;
  }
  .main {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
  }
  .name-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .name-row strong {
    color: var(--text);
  }
  .url {
    color: var(--muted);
    font-size: var(--text-xs);
    font-family: var(--mono);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .test {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: var(--text-xs);
    flex-wrap: wrap;
  }
  .test.ok {
    color: var(--green);
  }
  .test.err {
    color: var(--red);
  }
  .test code {
    color: var(--muted);
    font-size: 0.7rem;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 2px;
    flex-shrink: 0;
  }
  .icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    width: 28px;
    height: 28px;
    border-radius: var(--radius);
  }
  .icon:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .icon.danger:hover {
    color: var(--red);
  }
  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
    gap: var(--space-3);
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg, var(--radius));
    padding: var(--space-3);
    background: var(--surface);
  }
  /* Built-in gateway already on: keep the card present but visually settled. */
  .card.muted-card {
    opacity: 0.65;
  }
  .card-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .card-head strong {
    color: var(--text);
  }
  .desc {
    margin: 0;
    color: var(--muted);
    font-size: var(--text-sm);
    flex: 1;
  }
  .auth-note {
    display: flex;
    align-items: center;
    gap: 5px;
    margin: 0;
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .card-actions {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .docs {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--muted);
    font-size: var(--text-xs);
    text-decoration: none;
  }
  .docs:hover {
    text-decoration: underline;
  }
  .muted {
    color: var(--muted);
    font-size: var(--text-sm);
    margin: 0;
  }
  @media (max-width: 560px) {
    .grid2 {
      grid-template-columns: 1fr;
    }
  }
</style>
