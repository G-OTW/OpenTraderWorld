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
  import Badge from '$lib/ui/Badge.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import McpCallsPanel from '$lib/settings/McpCallsPanel.svelte';
  import McpTokenForm from '$lib/settings/McpTokenForm.svelte';

  let enabled = $state(false);
  let modules = $state([]);
  let tokens = $state([]);
  let loading = $state(true);
  let error = $state('');
  let toggling = $state(false);
  let oauthEnabled = $state(false);
  let oauthToggling = $state(false);

  let search = $state('');
  let sortDir = $state(1); // clicking the Name header flips A-Z / Z-A

  const shown = $derived.by(() => {
    const q = search.trim().toLowerCase();
    const out = q ? tokens.filter((t) => t.name.toLowerCase().includes(q)) : [...tokens];
    return out.sort((a, b) => a.name.localeCompare(b.name) * sortDir);
  });

  // Create/edit modal (shared with the automator, which mints a workflow token in place)
  let tokenForm = $state(null);

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
      oauthEnabled = !!s.oauth_enabled;
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

  async function toggleOauth() {
    oauthToggling = true;
    try {
      const r = await settingsApi.setMcpOauth(!oauthEnabled);
      oauthEnabled = r.oauth_enabled;
    } catch (e) {
      error = e.message;
    } finally {
      oauthToggling = false;
    }
  }

  async function onTokenSaved(r, edited) {
    if (!edited) {
      createdToken = r.token;
      createdOpen = true;
    }
    await reload();
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
  const isExpired = (tok) => !!tok.expires_at && new Date(tok.expires_at) <= new Date();
  const fmtExpiry = (tok) =>
    tok.expires_at ? fmtDate(tok.expires_at) : $t('settings.mcp.expiryNever');

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

  <input
    class="search"
    type="search"
    placeholder={$t('settings.mcp.searchPlaceholder')}
    aria-label={$t('settings.mcp.searchPlaceholder')}
    bind:value={search}
  />

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
            <th>{$t('settings.mcp.colExpires')}</th>
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
              <td><Skeleton height="0.85rem" width="60%" /></td>
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
  {:else if !shown.length}
    <EmptyState icon="search" compact title={$t('settings.mcp.noMatch')} />
  {:else}
    <div class="tablewrap">
      <table>
        <thead>
          <tr>
            <th>
              <button class="sort" onclick={() => (sortDir = -sortDir)}>
                {$t('settings.mcp.colName')}
                <Icon name={sortDir === 1 ? 'chevron-up' : 'chevron-down'} size={11} />
              </button>
            </th>
            <th>{$t('settings.mcp.colToken')}</th>
            <th>{$t('settings.mcp.colPermissions')}</th>
            <th>{$t('settings.mcp.colCreated')}</th>
            <th>{$t('settings.mcp.colLastUsed')}</th>
            <th>{$t('settings.mcp.colExpires')}</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each shown as tok (tok.id)}
            <tr>
              <td class="strong">
                {tok.name}
                {#if tok.external}
                  <Badge tone="accent" title={$t('settings.mcp.externalHint')}>
                    {$t('settings.mcp.externalTag')}
                  </Badge>
                {/if}
                {#if tok.oauth_client_id}
                  <Badge title={$t('settings.mcp.oauthTagHint')}>OAuth</Badge>
                {/if}
              </td>
              <td class="mono" data-label={$t('settings.mcp.colToken')} title={$t('common.clickToCopy')} use:copyLog={`${tok.prefix}…`}>{tok.prefix}…</td>
              <td data-label={$t('settings.mcp.colPermissions')}>{permSummary(tok.permissions)}</td>
              <td data-label={$t('settings.mcp.colCreated')}>{fmtDate(tok.created_at)}</td>
              <td data-label={$t('settings.mcp.colLastUsed')}>{fmtTime(tok.last_used_at)}</td>
              <td data-label={$t('settings.mcp.colExpires')} class:expired={isExpired(tok)}>
                {fmtExpiry(tok)}{#if isExpired(tok)} · {$t('settings.mcp.expired')}{/if}
              </td>
              <td class="actions">
                <button class="ghost" onclick={() => tokenForm?.openEdit(tok)}>
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

  <button class="primary" onclick={() => tokenForm?.openCreate()}>
    <Icon name="plus" size={13} /> {$t('settings.mcp.newToken')}
  </button>

  <h3>{$t('settings.mcp.callsTitle')}</h3>
  <p class="muted small">{$t('settings.mcp.callsHint')}</p>
  <McpCallsPanel />

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

  <div class="head oauth-head">
    <h3>{$t('settings.mcp.oauthTitle')}</h3>
    <label class="switch" class:on={oauthEnabled}>
      <input type="checkbox" checked={oauthEnabled} disabled={oauthToggling || loading} onchange={toggleOauth} />
      {oauthEnabled ? $t('settings.mcp.enabled') : $t('settings.mcp.disabled')}
    </label>
  </div>
  <p class="muted small">{$t('settings.mcp.oauthHint')}</p>
  {#if oauthEnabled}
    <p class="muted small">{$t('settings.mcp.oauthUrl')}</p>
    <CommandBlock command={endpoint} />
  {/if}
</div>

<McpTokenForm bind:this={tokenForm} {modules} onsaved={onTokenSaved} />

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
    font-size: 13.5px;
    font-weight: var(--fw-medium);
    letter-spacing: 0.02em;
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
  .oauth-head {
    margin-top: var(--space-6);
  }
  .oauth-head h3 {
    margin: 0;
  }
  .tabs {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
    margin: var(--space-2) 0;
  }
  .tab {
    background: transparent;
    border: var(--hairline) solid var(--border-control);
    border-bottom: 1.5px solid transparent;
    color: var(--muted);
    border-radius: 0;
    padding: 4px 10px;
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .tab:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .tab.active {
    color: var(--text);
    border-bottom-color: var(--accent);
    background: var(--surface-2);
  }
  .muted {
    color: var(--dim);
  }
  .small {
    font-size: 11.5px;
  }
  .banner {
    border: var(--hairline) solid var(--border);
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
  .search {
    width: 100%;
    max-width: 280px;
    margin-top: var(--space-3);
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
    border: var(--hairline) solid var(--border);
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
    border-bottom: var(--hairline) solid var(--border);
    white-space: nowrap;
  }
  tbody tr:last-child td {
    border-bottom: none;
  }
  th {
    color: var(--dim);
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    font-weight: var(--fw-medium);
    background: var(--surface-2);
  }
  .strong {
    font-weight: var(--fw-medium);
  }
  .mono {
    font-family: var(--mono);
    color: var(--muted);
  }
  /* An expired token is not an error, it is a token that did its job: amber, not red. */
  td.expired {
    color: var(--amber);
  }

  .actions {
    text-align: right;
  }
  button.ghost {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: transparent;
    border: var(--hairline) solid var(--border);
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
    background: transparent;
    color: var(--text);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    padding: 7px 14px;
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    letter-spacing: 0.04em;
    cursor: pointer;
  }
  button.primary:hover:not(:disabled) {
    background: var(--surface-2);
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
  /* Phone: each token is a card with its labels inline; Edit and Revoke stay on screen
     and the connect-example tabs wrap their own label. */
  @media (max-width: 600px) {
    .tablewrap:not([aria-busy]) {
      overflow-x: visible;
    }
    .tablewrap:not([aria-busy]) :is(table, tbody, tr, td) {
      display: block;
      width: 100%;
    }
    .tablewrap:not([aria-busy]) thead {
      display: none;
    }
    .tablewrap:not([aria-busy]) tr {
      padding: var(--space-2) 0;
      border-bottom: var(--hairline) solid var(--border);
    }
    .tablewrap:not([aria-busy]) tr:last-child {
      border-bottom: 0;
    }
    .tablewrap:not([aria-busy]) td {
      position: relative;
      border: 0;
      padding: var(--space-1) var(--space-3);
      white-space: normal;
      overflow-wrap: anywhere;
    }
    .tablewrap:not([aria-busy]) td[data-label] {
      padding-left: calc(38% + var(--space-3));
    }
    .tablewrap:not([aria-busy]) td[data-label]::before {
      content: attr(data-label);
      position: absolute;
      left: var(--space-3);
      width: calc(38% - var(--space-3));
      color: var(--dim);
      font-family: var(--font);
      font-size: var(--text-xs);
    }
    .tablewrap:not([aria-busy]) td.actions {
      display: flex;
      flex-wrap: wrap;
      justify-content: flex-end;
      gap: var(--space-2);
    }
    .actions button {
      min-height: 44px;
    }
    .tab {
      max-width: 100%;
      min-height: 44px;
      white-space: normal;
      text-align: left;
    }
  }
</style>
