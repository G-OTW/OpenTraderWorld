<script>
  // External control: chat channels that drive OTW back.
  //
  // A binding is (channel, agent, access token, allowed senders). It adds no permission
  // layer of its own: the token's per-module levels are the ceiling, and a token has to
  // carry "external access" before it can be picked here at all. The bot token asked for
  // below is the inbound credential (a Slack or Discord webhook URL can only send).
  import { onMount } from 'svelte';
  import { settingsApi } from '$lib/settings/api.js';
  import { channelsApi } from '$lib/notifications/api.js';
  import { agentApi } from '$lib/modules/agent/api.js';
  import ConfigField from '$lib/notifications/ConfigField.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import CommandBlock from '$lib/settings/CommandBlock.svelte';
  import { fmtDateTime } from '$lib/format.js';
  import { t } from '$lib/i18n';

  let enabled = $state(false);
  let kinds = $state([]);
  let pairTtl = $state(60);
  let pairTtlMin = $state(5);
  let pairTtlMax = $state(1440);
  let pairTtlDraft = $state('60');
  let savingTtl = $state(false);
  let bindings = $state([]);
  let channels = $state([]);
  let agents = $state([]);
  let providers = $state([]);
  let tokens = $state([]);
  let loading = $state(true);
  let error = $state('');
  let toggling = $state(false);

  let search = $state('');
  let sortDir = $state(1); // clicking the Name header flips A-Z / Z-A

  const shown = $derived.by(() => {
    const q = search.trim().toLowerCase();
    const out = q ? bindings.filter((b) => b.name.toLowerCase().includes(q)) : [...bindings];
    return out.sort((a, b) => a.name.localeCompare(b.name) * sortDir);
  });

  // Create/edit
  let modalOpen = $state(false);
  let editing = $state(null);
  let formName = $state('');
  let formChannel = $state('');
  let formAgent = $state('');
  let formToken = $state('');
  let formProvider = $state('');
  let formModel = $state('');
  let models = $state([]);
  let modelsError = $state('');
  let loadingModels = $state(false);
  let formSecret = $state('');
  let formVaultItem = $state(null);
  let formEnabled = $state(false);
  let formError = $state('');
  let saving = $state(false);

  // Pairing
  let pairOpen = $state(false);
  let pairCode = $state('');
  let pairFor = $state(null);

  let confirmDelete = $state('');

  // Only a channel that can receive is worth offering, and only one binding per channel.
  const eligible = $derived(
    channels.filter(
      (c) =>
        kinds.includes(c.kind) &&
        (c.id === editing?.channel_id || !bindings.some((b) => b.channel_id === c.id))
    )
  );
  // Same for tokens: external access is the opt-in that makes one selectable here.
  const externalTokens = $derived(
    tokens.filter(
      (tk) => tk.external && (tk.id === editing?.token_id || !bindings.some((b) => b.token_id === tk.id))
    )
  );

  onMount(reload);

  async function reload() {
    loading = true;
    error = '';
    try {
      const [s, list, chans, ags, provs, toks] = await Promise.all([
        settingsApi.controlSettings(),
        settingsApi.controlBindings(),
        channelsApi.list(),
        agentApi.listAgents(),
        agentApi.listProviders(),
        settingsApi.mcpTokens()
      ]);
      enabled = s.enabled;
      kinds = s.kinds ?? [];
      pairTtl = s.pair_ttl_minutes ?? 60;
      pairTtlMin = s.pair_ttl_min ?? 5;
      pairTtlMax = s.pair_ttl_max ?? 1440;
      pairTtlDraft = String(pairTtl);
      providers = (provs ?? []).filter((p) => p.enabled);
      bindings = list ?? [];
      channels = chans ?? [];
      // /agent/agents returns { agent, effective_skills } per row; only the persona is needed.
      agents = (ags ?? []).map((a) => a.agent ?? a);
      tokens = toks ?? [];
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  // The provider a binding would actually run on: its own override, then the persona's,
  // then the default agent's — the same fallback the server applies.
  const effectiveProvider = $derived.by(() => {
    if (formProvider) return formProvider;
    const persona = agents.find((a) => a.id === formAgent);
    if (persona?.provider_id) return persona.provider_id;
    return agents.find((a) => a.is_default)?.provider_id ?? '';
  });

  // Model ids come from the provider itself (queried server-side, the key never travels).
  $effect(() => {
    const id = effectiveProvider;
    if (!modalOpen || !id) {
      models = [];
      modelsError = '';
      return;
    }
    let stale = false;
    loadingModels = true;
    modelsError = '';
    agentApi
      .listProviderModels(id)
      .then((m) => {
        if (!stale) models = m ?? [];
      })
      .catch((e) => {
        if (!stale) {
          models = [];
          modelsError = e.message;
        }
      })
      .finally(() => {
        if (!stale) loadingModels = false;
      });
    return () => {
      stale = true;
    };
  });

  async function saveTtl() {
    const mins = Number(pairTtlDraft);
    if (!Number.isFinite(mins) || mins < pairTtlMin || mins > pairTtlMax) {
      error = $t('settings.control.err.ttlRange', { min: pairTtlMin, max: pairTtlMax });
      pairTtlDraft = String(pairTtl);
      return;
    }
    if (mins === pairTtl) return;
    savingTtl = true;
    error = '';
    try {
      const r = await settingsApi.setControlEnabled(enabled, mins);
      pairTtl = r.pair_ttl_minutes ?? mins;
      pairTtlDraft = String(pairTtl);
    } catch (e) {
      error = e.message;
      pairTtlDraft = String(pairTtl);
    } finally {
      savingTtl = false;
    }
  }

  async function toggleEnabled() {
    toggling = true;
    try {
      const r = await settingsApi.setControlEnabled(!enabled);
      enabled = r.enabled;
    } catch (e) {
      error = e.message;
    } finally {
      toggling = false;
    }
  }

  function openCreate() {
    editing = null;
    formName = '';
    formChannel = '';
    formAgent = agents.find((a) => a.is_default)?.id ?? agents[0]?.id ?? '';
    formToken = '';
    formProvider = '';
    formModel = '';
    formSecret = '';
    formVaultItem = null;
    formEnabled = false;
    formError = '';
    modalOpen = true;
  }

  function openEdit(b) {
    editing = b;
    formName = b.name;
    formChannel = b.channel_id;
    formAgent = b.agent_id;
    formToken = b.token_id;
    formProvider = b.provider_id ?? '';
    formModel = b.model ?? '';
    // Blank means "keep the stored credential"; the placeholder says so.
    formSecret = '';
    formVaultItem = b.secret_vault_item ?? null;
    formEnabled = b.enabled;
    formError = '';
    modalOpen = true;
  }

  async function save() {
    if (!formName.trim()) {
      formError = $t('settings.control.err.nameRequired');
      return;
    }
    if (!formChannel || !formAgent || !formToken) {
      formError = $t('settings.control.err.pickAll');
      return;
    }
    saving = true;
    formError = '';
    try {
      const payload = {
        name: formName.trim(),
        channel_id: formChannel,
        agent_id: formAgent,
        token_id: formToken,
        // Empty means "inherit from the agent", and the server stores that as NULL/''.
        provider_id: formProvider || null,
        model: formModel,
        enabled: formEnabled,
        // A vault reference wins; a pasted value is the fallback; neither means "keep".
        ...(formVaultItem
          ? { secret_vault_item: formVaultItem }
          : formSecret
            ? { secret: formSecret }
            : {})
      };
      if (editing) await settingsApi.updateControlBinding(editing.id, payload);
      else await settingsApi.createControlBinding(payload);
      modalOpen = false;
      await reload();
    } catch (e) {
      formError = e.message;
    } finally {
      saving = false;
    }
  }

  async function remove(b) {
    if (confirmDelete !== b.id) {
      confirmDelete = b.id;
      return;
    }
    confirmDelete = '';
    try {
      await settingsApi.deleteControlBinding(b.id);
      await reload();
    } catch (e) {
      error = e.message;
    }
  }

  async function pair(b) {
    try {
      const r = await settingsApi.pairControlBinding(b.id);
      pairCode = r.code;
      pairFor = b;
      pairOpen = true;
      await reload();
    } catch (e) {
      error = e.message;
    }
  }

  async function unpair(b, senderId) {
    try {
      await settingsApi.unpairControlSender(b.id, senderId);
      await reload();
    } catch (e) {
      error = e.message;
    }
  }

  const channelLabel = (id) => {
    const c = channels.find((x) => x.id === id);
    return c ? `${c.name} · ${c.kind}` : '—';
  };
  const agentLabel = (id) => agents.find((a) => a.id === id)?.name ?? '—';
  const modelLabel = (b) => {
    const p = providers.find((x) => x.id === b.provider_id);
    if (!p && !b.model) return '';
    return [p ? p.label : null, b.model || null].filter(Boolean).join(' · ');
  };
  const tokenLabel = (id) => tokens.find((tk) => tk.id === id)?.name ?? '—';
  const fmtTime = (iso) => {
    if (!iso) return '—';
    try {
      return fmtDateTime(iso);
    } catch {
      return iso;
    }
  };
</script>

<div class="section">
  <header class="head">
    <div class="heading">
      <h2>{$t('settings.control.title')}</h2>
      <p class="muted small">{$t('settings.control.subtitle')}</p>
    </div>
    <div class="head-controls">
      <label class="ttl">
        <span class="muted small">{$t('settings.control.pairTtl')}</span>
        <input
          type="number"
          min={pairTtlMin}
          max={pairTtlMax}
          step="5"
          bind:value={pairTtlDraft}
          disabled={savingTtl || loading}
          onblur={saveTtl}
        />
      </label>
      <label class="switch" class:on={enabled}>
        <input type="checkbox" checked={enabled} disabled={toggling || loading} onchange={toggleEnabled} />
        <span>{enabled ? $t('settings.control.enabled') : $t('settings.control.disabled')}</span>
      </label>
    </div>
  </header>

  <ErrorText {error} />

  {#if !enabled && !loading}
    <div class="banner">
      <span class="banner-icon" aria-hidden="true"><Icon name="info" size={16} /></span>
      <p>{$t('settings.control.offNote')}</p>
    </div>
  {/if}

  <section class="bindings-card">
    <div class="card-head">
      <h3>{$t('settings.control.colName')}</h3>
      <input
        class="search"
        type="search"
        placeholder={$t('settings.control.searchPlaceholder')}
        aria-label={$t('settings.control.searchPlaceholder')}
        bind:value={search}
      />
      <button class="primary" onclick={openCreate}>
        <Icon name="plus" size={13} /> {$t('settings.control.newBinding')}
      </button>
    </div>

    {#if loading}
      <div class="tablewrap" aria-busy="true">
        <table>
          <thead>
            <tr>
              <th>{$t('settings.control.colName')}</th>
              <th>{$t('settings.control.colChannel')}</th>
              <th>{$t('settings.control.colAgent')}</th>
              <th>{$t('settings.control.colToken')}</th>
              <th>{$t('settings.control.colModel')}</th>
              <th>{$t('settings.control.colSenders')}</th>
              <th>{$t('settings.control.colStatus')}</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {#each Array.from({ length: 2 }, (_, i) => i) as i (i)}
              <tr>
                {#each Array.from({ length: 8 }, (_, j) => j) as j (j)}
                  <td><Skeleton height="0.85rem" width="70%" /></td>
                {/each}
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {:else if !bindings.length}
      <div class="empty-wrap">
        <EmptyState icon="message-square" title={$t('settings.control.none')} />
      </div>
    {:else if !shown.length}
      <div class="empty-wrap">
        <EmptyState icon="search" title={$t('settings.control.noMatch')} />
      </div>
    {:else}
      <div class="tablewrap">
        <table>
          <thead>
            <tr>
              <th>
                <button class="sort" onclick={() => (sortDir = -sortDir)}>
                  {$t('settings.control.colName')}
                  <Icon name={sortDir === 1 ? 'chevron-up' : 'chevron-down'} size={11} />
                </button>
              </th>
              <th>{$t('settings.control.colChannel')}</th>
              <th>{$t('settings.control.colAgent')}</th>
              <th>{$t('settings.control.colToken')}</th>
              <th>{$t('settings.control.colModel')}</th>
              <th>{$t('settings.control.colSenders')}</th>
              <th>{$t('settings.control.colStatus')}</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {#each shown as b (b.id)}
              <tr>
                <td class="strong">
                  {b.name}
                  {#if !b.enabled}
                    <Badge>{$t('settings.control.off')}</Badge>
                  {/if}
                </td>
                <td>{b.channel_name} · {b.channel_kind}</td>
                <td>{agentLabel(b.agent_id)}</td>
                <td>{tokenLabel(b.token_id)}</td>
                <td>
                  {#if modelLabel(b)}
                    {modelLabel(b)}
                  {:else}
                    <span class="muted">{$t('settings.control.inherited')}</span>
                  {/if}
                </td>
                <td>
                  {#if !b.senders.length}
                    <span class="muted">{$t('settings.control.noSenders')}</span>
                  {:else}
                    <span class="senders">
                      {#each b.senders as s (s.sender_id)}
                        <button class="sender" onclick={() => unpair(b, s.sender_id)} title={$t('settings.control.unpair')}>
                          {s.label || s.sender_id} <Icon name="x" size={10} />
                        </button>
                      {/each}
                    </span>
                  {/if}
                </td>
                <td>
                  {#if !b.enabled}
                    <span class="muted">—</span>
                  {:else if b.last_ok === true}
                    <Badge tone="success">{$t('settings.control.connected')}</Badge>
                    <span class="muted small">{fmtTime(b.last_seen_at)}</span>
                  {:else if b.last_ok === false}
                    <Badge tone="danger" title={b.last_error ?? ''}>{$t('settings.control.failing')}</Badge>
                  {:else}
                    <span class="muted">{$t('settings.control.starting')}</span>
                  {/if}
                </td>
                <td class="actions">
                  <button class="ghost" onclick={() => pair(b)}>
                    <Icon name="link" size={12} /> {$t('settings.control.pair')}
                  </button>
                  <button class="ghost" onclick={() => openEdit(b)}>
                    <Icon name="pencil" size={12} /> {$t('common.edit')}
                  </button>
                  <button class="ghost danger" class:armed={confirmDelete === b.id} onclick={() => remove(b)}>
                    <Icon name="trash" size={12} />
                    {confirmDelete === b.id ? $t('settings.control.deleteConfirm') : $t('common.delete')}
                  </button>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </section>

  <aside class="notes-card">
    <span class="notes-icon" aria-hidden="true"><Icon name="shield" size={17} /></span>
    <ul class="notes muted small">
      <li>{$t('settings.control.note1')}</li>
      <li>{$t('settings.control.note2')}</li>
      <li>{$t('settings.control.note3')}</li>
    </ul>
  </aside>
</div>

<Modal
  bind:open={modalOpen}
  title={editing ? $t('settings.control.editTitle') : $t('settings.control.newBinding')}
  size="md"
>
  <div class="form">
    <label class="field">
      <span>{$t('settings.control.name')}</span>
      <input type="text" bind:value={formName} placeholder={$t('settings.control.namePlaceholder')} maxlength="80" />
    </label>

    <label class="field">
      <span>{$t('settings.control.channel')}</span>
      <Dropdown
        value={formChannel}
        onpick={(v) => (formChannel = v)}
        ariaLabel={$t('settings.control.channel')}
        options={eligible.map((c) => ({ value: c.id, label: `${c.name} · ${c.kind}` }))}
      />
    </label>
    {#if !eligible.length}
      <p class="muted small">{$t('settings.control.noChannels', { kinds: kinds.join(', ') })}</p>
    {/if}

    <label class="field">
      <span>{$t('settings.control.agent')}</span>
      <Dropdown
        value={formAgent}
        onpick={(v) => (formAgent = v)}
        ariaLabel={$t('settings.control.agent')}
        options={agents.map((a) => ({ value: a.id, label: a.name }))}
      />
    </label>

    <label class="field">
      <span>{$t('settings.control.token')}</span>
      <Dropdown
        value={formToken}
        onpick={(v) => (formToken = v)}
        ariaLabel={$t('settings.control.token')}
        options={externalTokens.map((tk) => ({ value: tk.id, label: tk.name }))}
      />
    </label>
    {#if !externalTokens.length}
      <p class="muted small">{$t('settings.control.noTokens')}</p>
    {/if}

    <label class="field">
      <span>{$t('settings.control.provider')}</span>
      <Dropdown
        value={formProvider}
        onpick={(v) => {
          // A model id belongs to one vendor: changing provider drops it.
          if (v !== formProvider) formModel = '';
          formProvider = v;
        }}
        ariaLabel={$t('settings.control.provider')}
        options={[
          { value: '', label: $t('settings.control.inheritProvider') },
          ...providers.map((p) => ({ value: p.id, label: `${p.label} · ${p.kind}` }))
        ]}
      />
    </label>

    <label class="field">
      <span>{$t('settings.control.model')}</span>
      <Dropdown
        value={formModel}
        onpick={(v) => (formModel = v)}
        ariaLabel={$t('settings.control.model')}
        options={[
          { value: '', label: $t('settings.control.inheritModel') },
          ...models.map((m) => ({ value: m, label: m })),
          ...(formModel && !models.includes(formModel)
            ? [{ value: formModel, label: formModel }]
            : [])
        ]}
      />
    </label>
    {#if loadingModels}
      <p class="muted small">{$t('settings.control.loadingModels')}</p>
    {:else if modelsError}
      <p class="muted small">{modelsError}</p>
    {/if}
    <p class="muted small">{$t('settings.control.modelHint')}</p>

    <div class="field">
      <span>{$t('settings.control.secret')}</span>
      <ConfigField
        bind:value={formSecret}
        bind:vaultItem={formVaultItem}
        type="password"
        placeholder={editing?.has_secret
          ? $t('settings.control.secretKeep')
          : $t('settings.control.secretPlaceholder')}
      />
    </div>
    <p class="muted small">{$t('settings.control.secretHint')}</p>

    <label class="check">
      <input type="checkbox" bind:checked={formEnabled} />
      <span>{$t('settings.control.enable')}</span>
    </label>

    <ErrorText error={formError} />
  </div>
  {#snippet footer()}
    <button class="ghost" onclick={() => (modalOpen = false)}>{$t('common.cancel')}</button>
    <button class="primary" disabled={saving} onclick={save}>
      {saving ? $t('common.saving') : $t('common.save')}
    </button>
  {/snippet}
</Modal>

<Modal bind:open={pairOpen} title={$t('settings.control.pairTitle')} size="sm" onclose={() => (pairCode = '')}>
  <p class="muted small">{$t('settings.control.pairBody', { name: pairFor?.name ?? '', minutes: pairTtl })}</p>
  <CommandBlock command={pairCode} />
  <p class="muted small">{$t('settings.control.pairNote')}</p>
</Modal>

<style>
  .section {
    display: flex;
    width: min(100%, 1180px);
    flex-direction: column;
    gap: var(--space-4);
  }
  .section h2 {
    margin: 0;
    color: var(--text);
    font-size: var(--fs-item-title);
    font-weight: var(--fw-medium);
    letter-spacing: 0.02em;
  }
  .head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-6);
  }
  .heading {
    display: flex;
    max-width: 82ch;
    flex-direction: column;
    gap: 6px;
  }
  .heading p {
    margin: 0;
  }
  .muted {
    color: var(--dim);
  }
  .small {
    font-size: var(--fs-desc);
    line-height: 1.5;
  }
  .banner {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    border: var(--hairline) solid var(--border);
    border-left: var(--active-rule) solid var(--amber);
    border-radius: var(--radius);
    background: var(--surface-2);
    padding: 11px 14px;
    color: var(--muted);
  }
  .banner p {
    margin: 0;
    font-size: var(--text-sm);
    line-height: 1.5;
  }
  .banner-icon {
    display: inline-flex;
    margin-top: 2px;
    flex: none;
    color: var(--amber-ink);
  }
  .switch {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 116px;
    min-height: 42px;
    justify-content: center;
    flex: none;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    background: var(--surface);
    padding: 0 var(--space-3);
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    color: var(--muted);
    cursor: pointer;
  }
  .head-controls {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex: none;
  }
  .ttl {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    white-space: nowrap;
  }
  .ttl input {
    width: 86px;
  }
  .switch.on {
    border-color: color-mix(in srgb, var(--green) 45%, var(--border-control));
    background: color-mix(in srgb, var(--green) 7%, var(--surface));
    color: var(--green-ink);
  }
  .switch:has(input:disabled) {
    cursor: default;
    opacity: 0.65;
  }
  .bindings-card {
    overflow: hidden;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-lg);
    background: var(--surface);
  }
  .card-head {
    display: flex;
    min-height: 60px;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
  }
  .card-head h3 {
    margin: 0;
    color: var(--text);
    font-size: var(--text-base);
    font-weight: var(--fw-medium);
  }
  .card-head .search {
    margin-left: auto;
    width: 100%;
    max-width: 240px;
  }
  .sort {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    background: none;
    border: none;
    padding: 0;
    color: inherit;
    font: inherit;
    text-transform: inherit;
    letter-spacing: inherit;
    cursor: pointer;
  }
  .sort:hover {
    color: var(--text);
  }
  .tablewrap {
    overflow-x: auto;
    border-top: var(--hairline) solid var(--border);
  }
  table {
    width: 100%;
    min-width: 1200px;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }
  th,
  td {
    padding: 10px 12px;
    border-bottom: var(--hairline) solid var(--border);
    text-align: left;
    vertical-align: middle;
    white-space: nowrap;
  }
  th {
    background: var(--surface-2);
    color: var(--dim);
    font-size: var(--fs-metric-label);
    font-weight: var(--fw-medium);
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  tbody tr:last-child td {
    border-bottom: 0;
  }
  tbody tr:hover td {
    background: color-mix(in srgb, var(--surface-2) 58%, transparent);
  }
  .empty-wrap {
    min-height: 220px;
    border-top: var(--hairline) solid var(--border);
    display: grid;
    place-items: center;
  }
  .strong {
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  .senders {
    display: inline-flex;
    flex-wrap: wrap;
    gap: var(--space-1);
  }
  .sender {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    padding: 4px 7px;
    color: var(--muted);
    font-size: var(--text-xs);
    cursor: pointer;
  }
  .sender:hover {
    border-color: var(--border-strong);
    color: var(--text);
  }
  .actions {
    text-align: right;
    white-space: nowrap;
  }
  .actions button + button {
    margin-left: 2px;
  }
  .actions button.armed {
    border-color: color-mix(in srgb, var(--red) 50%, var(--border-control));
    color: var(--red);
  }
  .notes-card {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--surface-2) 72%, var(--surface));
    padding: var(--space-3) var(--space-4);
  }
  .notes-icon {
    display: inline-flex;
    width: 34px;
    height: 34px;
    align-items: center;
    justify-content: center;
    flex: none;
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface);
    color: var(--muted);
  }
  .notes {
    display: flex;
    flex-direction: column;
    gap: 5px;
    margin: 1px 0 0;
    padding-left: 18px;
  }
  .notes li::marker {
    color: var(--faint);
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 5px;
    font-size: var(--text-base);
  }
  .check {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-base);
  }
  .check input {
    width: auto;
  }
  @media (max-width: 760px) {
    .head {
      flex-direction: column;
      gap: var(--space-3);
    }
    .head-controls {
      align-self: flex-start;
      flex-wrap: wrap;
    }
    .switch {
      min-width: 0;
    }
    .card-head {
      min-height: 0;
      align-items: flex-start;
      flex-direction: column;
    }
    .notes-card {
      padding: var(--space-3);
    }
  }
</style>
