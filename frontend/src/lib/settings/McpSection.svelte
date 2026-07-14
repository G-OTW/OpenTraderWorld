<script>
  // MCP (AI agents) settings: global endpoint toggle + bearer-token management.
  // Tokens are shown in plaintext exactly once (on create); the backend stores a hash.
  // Each token carries per-module permissions: absent = no access, 'r' = read,
  // 'rw' = read+write (no delete), 'rwd' = full (read+write+delete).
  import { onMount } from 'svelte';
  import { settingsApi } from '$lib/settings/api.js';
  import { copyLog } from '$lib/ui/copyLog.js';
  import CommandBlock from '$lib/settings/CommandBlock.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';
  import { fmtDateTime } from '$lib/format.js';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';

  let enabled = $state(false);
  let modules = $state([]);
  let tokens = $state([]);
  let loading = $state(true);
  let error = $state('');
  let toggling = $state(false);

  // Create/edit modal
  let modalOpen = $state(false);
  let editing = $state(null);
  let formName = $state('');
  let formPerms = $state({});
  let formError = $state('');
  let saving = $state(false);

  // One-time plaintext display after create
  let createdOpen = $state(false);
  let createdToken = $state('');

  // Two-step revoke (first click arms, second deletes)
  let confirmRevoke = $state('');

  const endpoint =
    typeof location !== 'undefined' ? `${location.origin}/api/mcp` : '/api/mcp';

  // Connection examples, one per MCP client family. The endpoint speaks MCP over
  // Streamable HTTP + Bearer auth, so any compliant client works; these are the
  // common ones. `token` is <TOKEN> as a placeholder, or the real value after create.
  function connectExamples(token) {
    const bearer = `Bearer ${token}`;
    return [
      {
        id: 'raw',
        label: $t('settings.mcp.connectRaw'),
        command: `${endpoint}\nAuthorization: ${bearer}`,
      },
      {
        id: 'json',
        // Works with Cursor, Cline, Windsurf, VS Code, and most MCP clients that
        // read an mcpServers JSON block.
        label: 'JSON config (Cursor, Cline, Windsurf, VS Code…)',
        command: JSON.stringify(
          {
            mcpServers: {
              opentraderworld: {
                type: 'http',
                url: endpoint,
                headers: { Authorization: bearer },
              },
            },
          },
          null,
          2,
        ),
      },
      {
        id: 'cli',
        label: 'Claude Code / CLI',
        command: `claude mcp add --transport http opentraderworld ${endpoint} --header "Authorization: ${bearer}"`,
      },
    ];
  }

  let connectTab = $state('raw');
  const examples = $derived(connectExamples('<TOKEN>'));
  const activeExample = $derived(examples.find((e) => e.id === connectTab) ?? examples[0]);

  onMount(reload);

  async function reload() {
    loading = true;
    error = '';
    try {
      const [s, toks] = await Promise.all([settingsApi.mcpSettings(), settingsApi.mcpTokens()]);
      enabled = s.enabled;
      modules = s.modules ?? [];
      tokens = toks ?? [];
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  async function toggleEnabled() {
    toggling = true;
    try {
      const r = await settingsApi.setMcpEnabled(!enabled);
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
    formPerms = {};
    formError = '';
    modalOpen = true;
  }

  function openEdit(tok) {
    editing = tok;
    formName = tok.name;
    formPerms = { ...(tok.permissions ?? {}) };
    formError = '';
    modalOpen = true;
  }

  function setLevel(id, level) {
    const next = { ...formPerms };
    if (level) next[id] = level;
    else delete next[id];
    formPerms = next;
  }

  function setAll(level) {
    formPerms = level ? Object.fromEntries(modules.map((m) => [m.id, level])) : {};
  }

  async function save() {
    if (!formName.trim()) {
      formError = $t('settings.mcp.err.nameRequired');
      return;
    }
    saving = true;
    formError = '';
    try {
      if (editing) {
        await settingsApi.updateMcpToken(editing.id, { name: formName, permissions: formPerms });
      } else {
        const r = await settingsApi.createMcpToken({ name: formName, permissions: formPerms });
        createdToken = r.token;
        createdOpen = true;
      }
      modalOpen = false;
      await reload();
    } catch (e) {
      formError = e.message;
    } finally {
      saving = false;
    }
  }

  async function revoke(tok) {
    if (confirmRevoke !== tok.id) {
      confirmRevoke = tok.id;
      return;
    }
    confirmRevoke = '';
    try {
      await settingsApi.deleteMcpToken(tok.id);
      await reload();
    } catch (e) {
      error = e.message;
    }
  }

  function permSummary(perms) {
    const vals = Object.values(perms ?? {});
    if (!vals.length) return $t('settings.mcp.permNone');
    const full = vals.filter((v) => v === 'rwd').length;
    const write = vals.filter((v) => v === 'rw').length;
    return $t('settings.mcp.permSummary', {
      read: vals.length - write - full,
      write,
      full,
    });
  }

  const fmtDate = (iso) => {
    if (!iso) return '—';
    try {
      return new Date(iso).toLocaleDateString();
    } catch {
      return iso;
    }
  };
  // "Last used" — fmtDateTime drops the seconds the raw toLocaleString() prints, which
  // nobody reads on this column. (LogsSection and RateSection keep the raw call: there,
  // the seconds are the point.)
  const fmtTime = (iso) => {
    if (!iso) return $t('settings.mcp.never');
    try {
      return fmtDateTime(iso);
    } catch {
      return iso;
    }
  };
</script>

<div class="section">
  <div class="head">
    <h2>{$t('settings.mcp.title')}</h2>
    <label class="switch" class:on={enabled}>
      <input type="checkbox" checked={enabled} disabled={toggling || loading} onchange={toggleEnabled} />
      {enabled ? $t('settings.mcp.enabled') : $t('settings.mcp.disabled')}
    </label>
  </div>
  <p class="muted small">{$t('settings.mcp.subtitle')}</p>

  <ErrorText error={error} />

  {#if !enabled && !loading}
    <p class="banner">{$t('settings.mcp.offNote')}</p>
  {/if}

  <h3>{$t('settings.mcp.tokensTitle')}</h3>
  <p class="muted small">{$t('settings.mcp.tokensHint')}</p>

  {#if loading}
    <!-- The header is known before the fetch lands, so it stays and only the body is
         skeletoned; swapping the whole table for a text line collapses the columns. -->
    <div class="tablewrap" aria-busy="true">
      <table>
        <thead>
          <tr>
            <th>{$t('settings.mcp.colName')}</th>
            <th>{$t('settings.mcp.colToken')}</th>
            <th>{$t('settings.mcp.colPermissions')}</th>
            <th>{$t('settings.mcp.colCreated')}</th>
            <th>{$t('settings.mcp.colLastUsed')}</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each Array.from({ length: 3 }, (_, i) => i) as i (i)}
            <tr>
              <td><Skeleton height="0.85rem" width="70%" /></td>
              <td><Skeleton height="0.85rem" width="80%" /></td>
              <td><Skeleton height="0.85rem" width="50%" /></td>
              <td><Skeleton height="0.85rem" width="75%" /></td>
              <td><Skeleton height="0.85rem" width="75%" /></td>
              <td><Skeleton height="0.85rem" width="40%" /></td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else if !tokens.length}
    <!-- Truly empty, not filtered. The "New token" button sits directly below and the hint
         is already printed above, so the empty state only names the gap. -->
    <EmptyState icon="zap" compact title={$t('settings.mcp.noTokens')} />
  {:else}
    <div class="tablewrap">
      <table>
        <thead>
          <tr>
            <th>{$t('settings.mcp.colName')}</th>
            <th>{$t('settings.mcp.colToken')}</th>
            <th>{$t('settings.mcp.colPermissions')}</th>
            <th>{$t('settings.mcp.colCreated')}</th>
            <th>{$t('settings.mcp.colLastUsed')}</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each tokens as tok (tok.id)}
            <tr>
              <td class="strong">{tok.name}</td>
              <td class="mono" title={$t('common.clickToCopy')} use:copyLog={`${tok.prefix}…`}>{tok.prefix}…</td>
              <td>{permSummary(tok.permissions)}</td>
              <td>{fmtDate(tok.created_at)}</td>
              <td>{fmtTime(tok.last_used_at)}</td>
              <td class="actions">
                <button class="ghost" onclick={() => openEdit(tok)}>
                  <Icon name="pencil" size={12} /> {$t('settings.mcp.edit')}
                </button>
                <button class="ghost danger" class:armed={confirmRevoke === tok.id} onclick={() => revoke(tok)}>
                  <Icon name="trash" size={12} />
                  {confirmRevoke === tok.id ? $t('settings.mcp.revokeConfirm') : $t('settings.mcp.revoke')}
                </button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}

  <button class="primary" onclick={openCreate}>
    <Icon name="plus" size={13} /> {$t('settings.mcp.newToken')}
  </button>

  <h3>{$t('settings.mcp.connectTitle')}</h3>
  <p class="muted small">{$t('settings.mcp.connectHint')}</p>
  <div class="tabs">
    {#each examples as ex (ex.id)}
      <button
        class="tab"
        class:active={connectTab === ex.id}
        onclick={() => (connectTab = ex.id)}>{ex.label}</button
      >
    {/each}
  </div>
  <CommandBlock command={activeExample.command} />
  <ul class="notes muted small">
    <li>{$t('settings.mcp.note1')}</li>
    <li>{$t('settings.mcp.note2')}</li>
    <li>{$t('settings.mcp.note3')}</li>
  </ul>
</div>

<Modal bind:open={modalOpen} title={editing ? $t('settings.mcp.editTitle') : $t('settings.mcp.newToken')} size="md">
  <div class="form">
    <label class="field">
      <span>{$t('settings.mcp.name')}</span>
      <input type="text" bind:value={formName} placeholder={$t('settings.mcp.namePlaceholder')} maxlength="80" />
    </label>

    <div class="permhead">
      <span>{$t('settings.mcp.permissions')}</span>
      <span class="bulk">
        <button class="ghost" onclick={() => setAll('r')}>{$t('settings.mcp.allRead')}</button>
        <button class="ghost" onclick={() => setAll('rw')}>{$t('settings.mcp.allRw')}</button>
        <button class="ghost" onclick={() => setAll('rwd')}>{$t('settings.mcp.allFull')}</button>
        <button class="ghost" onclick={() => setAll('')}>{$t('common.clear')}</button>
      </span>
    </div>
    <div class="matrix">
      {#each modules as m (m.id)}
        <div class="permrow">
          <span class="mlabel">{m.label}</span>
          <select value={formPerms[m.id] ?? ''} onchange={(e) => setLevel(m.id, e.currentTarget.value)}>
            <option value="">{$t('settings.mcp.levelNone')}</option>
            <option value="r">{$t('settings.mcp.levelRead')}</option>
            <option value="rw">{$t('settings.mcp.levelRw')}</option>
            <option value="rwd">{$t('settings.mcp.levelFull')}</option>
          </select>
        </div>
      {/each}
    </div>

    <ErrorText error={formError} />
  </div>
  {#snippet footer()}
    <button class="ghost" onclick={() => (modalOpen = false)}>{$t('common.cancel')}</button>
    <button class="primary" disabled={saving} onclick={save}>
      {saving ? $t('common.saving') : editing ? $t('common.save') : $t('settings.mcp.create')}
    </button>
  {/snippet}
</Modal>

<Modal bind:open={createdOpen} title={$t('settings.mcp.createdTitle')} size="md" onclose={() => (createdToken = '')}>
  <p class="muted small">{$t('settings.mcp.createdBody')}</p>
  <CommandBlock command={createdToken} />
  <p class="muted small">{$t('settings.mcp.createdConfig')}</p>
  {@const created = connectExamples(createdToken)}
  <div class="tabs">
    {#each created as ex (ex.id)}
      <button
        class="tab"
        class:active={connectTab === ex.id}
        onclick={() => (connectTab = ex.id)}>{ex.label}</button
      >
    {/each}
  </div>
  <CommandBlock command={(created.find((e) => e.id === connectTab) ?? created[0]).command} />
</Modal>

<style>
  .section h2 {
    margin: 0;
    font-size: var(--text-md);
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    margin-bottom: var(--space-2);
  }
  h3 {
    margin: var(--space-6) 0 var(--space-2);
    font-size: var(--text-base);
  }
  .tabs {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
    margin: var(--space-2) 0;
  }
  .tab {
    background: var(--surface-2);
    border: 1px solid var(--border);
    color: var(--muted);
    border-radius: var(--radius);
    padding: 4px 10px;
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .tab:hover {
    color: var(--text);
  }
  .tab.active {
    color: var(--text);
    border-color: var(--accent);
    background: var(--surface);
  }
  .muted {
    color: var(--muted);
  }
  .small {
    font-size: var(--text-sm);
  }
  .banner {
    border: 1px solid var(--border);
    border-left: 3px solid var(--amber);
    background: var(--surface-2);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .switch {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-base);
    color: var(--muted);
    cursor: pointer;
  }
  .switch.on {
    color: var(--green-ink);
  }
  .tablewrap {
    overflow-x: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    margin-bottom: var(--space-3);
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }
  th,
  td {
    text-align: left;
    padding: 8px 10px;
    border-bottom: 1px solid var(--border);
    white-space: nowrap;
  }
  tbody tr:last-child td {
    border-bottom: none;
  }
  th {
    color: var(--muted);
    font-weight: var(--fw-medium);
    background: var(--surface-2);
  }
  .strong {
    font-weight: var(--fw-semibold);
  }
  .mono {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    color: var(--muted);
  }
  .actions {
    text-align: right;
  }
  button.ghost {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: transparent;
    border: 1px solid var(--border);
    color: var(--muted);
    border-radius: var(--radius);
    padding: 4px 10px;
    font-size: var(--text-sm);
    cursor: pointer;
  }
  button.ghost:hover {
    color: var(--text);
  }
  button.ghost.danger:hover,
  button.ghost.armed {
    color: var(--red-ink);
    border-color: var(--red);
  }
  button.primary {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    background: var(--accent);
    color: var(--accent-contrast);
    border: none;
    border-radius: var(--radius);
    padding: 7px 14px;
    font-size: var(--text-sm);
    cursor: pointer;
  }
  button.primary:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .notes {
    margin: var(--space-2) 0 0;
    padding-left: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: var(--text-base);
  }
  .field input {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    padding: 7px 10px;
    font-size: var(--text-base);
  }
  .permhead {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: var(--text-base);
  }
  .bulk {
    display: inline-flex;
    gap: var(--space-2);
  }
  .matrix {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    max-height: 320px;
    overflow-y: auto;
  }
  .permrow {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    padding: 6px 10px;
    border-bottom: 1px solid var(--border);
    font-size: var(--text-sm);
  }
  .permrow:last-child {
    border-bottom: none;
  }
  .permrow select {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    padding: 4px 8px;
    font-size: var(--text-sm);
  }
</style>
