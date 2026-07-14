<script>
  // Webhooks module. Inbound endpoints that redirect each received payload to an
  // existing module (v1 target: RemindMe — payload → notification). Sender-agnostic:
  // anything that can POST a small text/JSON body works. The endpoint URL embeds the
  // credential token, shown exactly once at creation.
  import { onMount } from 'svelte';
  import { webhooksApi } from '$lib/modules/webhooks/api.js';
  import { settingsApi } from '$lib/settings/api.js';
  import Modal from '$lib/ui/Modal.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { copyLog } from '$lib/ui/copyLog.js';
  import PageHeader from '$lib/ui/PageHeader.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import Input from '$lib/ui/Input.svelte';
  import Select from '$lib/ui/Select.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import CommandBlock from '$lib/settings/CommandBlock.svelte';
  import { t } from '$lib/i18n';
  import { fmtDateTime } from '$lib/format.js';

  let webhooks = $state([]);
  let targets = $state([]);
  let loading = $state(true);
  let error = $state('');
  // Network mode, to warn when external senders can't reach this host ('' until known).
  let netMode = $state('');

  // Create/edit modal
  let modalOpen = $state(false);
  let editing = $state(null);
  let formName = $state('');
  let formTarget = $state('remindme');
  let formError = $state('');
  let saving = $state(false);

  // One-time full-URL display after create
  let createdOpen = $state(false);
  let createdUrl = $state('');

  // Events viewer
  let eventsOpen = $state(false);
  let eventsFor = $state(null);
  let events = $state([]);
  let eventsLoading = $state(false);

  // Delete confirmation
  let confirmOpen = $state(false);
  let deleting = $state(null);

  const origin = typeof location !== 'undefined' ? location.origin : '';

  onMount(reload);

  async function reload() {
    loading = true;
    error = '';
    try {
      const r = await webhooksApi.list();
      webhooks = r.webhooks ?? [];
      targets = r.targets ?? [];
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
    // Best-effort: the warning is advisory, never block the page on it.
    try {
      netMode = (await settingsApi.getNetwork())?.mode ?? '';
    } catch {
      netMode = '';
    }
  }

  // Only 'web' is internet-reachable by itself; the other modes need a tunnel/relay.
  const netUnreachable = $derived(!!netMode && netMode !== 'web');

  // Target labels are i18n'd by id; the backend label is the fallback for targets
  // added before their translation lands.
  function targetLabel(id) {
    const key = `webhooks.target.${id}`;
    const label = $t(key);
    return label !== key ? label : (targets.find((x) => x.id === id)?.label ?? id);
  }
  const targetOptions = $derived(targets.map((x) => ({ value: x.id, label: targetLabel(x.id) })));

  function openCreate() {
    editing = null;
    formName = '';
    formTarget = targets[0]?.id ?? 'remindme';
    formError = '';
    modalOpen = true;
  }

  function openEdit(wh) {
    editing = wh;
    formName = wh.name;
    formTarget = wh.target;
    formError = '';
    modalOpen = true;
  }

  async function save() {
    if (!formName.trim()) {
      formError = $t('webhooks.err.nameRequired');
      return;
    }
    saving = true;
    formError = '';
    try {
      if (editing) {
        await webhooksApi.update(editing.id, { name: formName, target: formTarget });
      } else {
        const r = await webhooksApi.create({ name: formName, target: formTarget });
        createdUrl = `${origin}${r.path}`;
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

  async function toggleEnabled(wh) {
    try {
      const updated = await webhooksApi.update(wh.id, { enabled: !wh.enabled });
      webhooks = webhooks.map((w) => (w.id === wh.id ? updated : w));
    } catch (e) {
      error = e.message;
    }
  }

  function askDelete(wh) {
    deleting = wh;
    confirmOpen = true;
  }

  async function confirmDelete() {
    confirmOpen = false;
    if (!deleting) return;
    try {
      await webhooksApi.remove(deleting.id);
      await reload();
    } catch (e) {
      error = e.message;
    } finally {
      deleting = null;
    }
  }

  async function openEvents(wh) {
    eventsFor = wh;
    eventsOpen = true;
    eventsLoading = true;
    events = [];
    try {
      events = await webhooksApi.events(wh.id);
    } catch (e) {
      error = e.message;
    } finally {
      eventsLoading = false;
    }
  }

  const STATUS_TONE = { ok: 'success', error: 'danger', ignored: 'neutral' };

  const fmtTime = (iso) => {
    if (!iso) return $t('webhooks.never');
    try {
      return fmtDateTime(iso);
    } catch {
      return iso;
    }
  };
</script>

<div class="page">
  <PageHeader
    title={$t('webhooks.title')}
    subtitle={$t('webhooks.count', { count: webhooks.length, s: webhooks.length === 1 ? '' : 's' })}
  >
    {#snippet actions()}
      <Button variant="primary" icon="plus" onclick={openCreate}>{$t('webhooks.add')}</Button>
    {/snippet}
    <p class="hint">{$t('webhooks.hint')}</p>
  </PageHeader>

  <ErrorText error={error} />

  {#if netUnreachable}
    <div class="netwarn" role="status">
      <Icon name="alert-triangle" size={15} />
      <div>
        <p>{$t('webhooks.netWarn', { mode: netMode })}</p>
        <p>
          <a href="/settings#network">{$t('webhooks.netWarnLink')}</a>
          {$t('webhooks.netWarnOr')}
        </p>
      </div>
    </div>
  {/if}

  {#if loading}
    <div class="tablewrap" aria-busy="true">
      <table class="tbl">
        <tbody>
          {#each Array.from({ length: 3 }, (_, i) => i) as i (i)}
            <tr>
              <td><Skeleton height="0.85rem" width="70%" /></td>
              <td><Skeleton height="0.85rem" width="60%" /></td>
              <td><Skeleton height="0.85rem" width="80%" /></td>
              <td><Skeleton height="0.85rem" width="40%" /></td>
              <td><Skeleton height="0.85rem" width="60%" /></td>
              <td><Skeleton height="0.85rem" width="50%" /></td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else if !webhooks.length}
    <EmptyState
      icon="webhook"
      title={$t('webhooks.emptyTitle')}
      description={$t('webhooks.emptyBody')}
    >
      {#snippet action()}
        <Button variant="primary" icon="plus" onclick={openCreate}>{$t('webhooks.add')}</Button>
      {/snippet}
    </EmptyState>
  {:else}
    <div class="tablewrap">
      <table class="tbl">
        <thead>
          <tr>
            <th>{$t('webhooks.colName')}</th>
            <th>{$t('webhooks.colTarget')}</th>
            <th>{$t('webhooks.colUrl')}</th>
            <th class="num">{$t('webhooks.colReceived')}</th>
            <th>{$t('webhooks.colLast')}</th>
            <th>{$t('webhooks.colStatus')}</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each webhooks as wh (wh.id)}
            <tr class:off={!wh.enabled}>
              <td class="strong">{wh.name}</td>
              <td>{targetLabel(wh.target)}</td>
              <td
                class="mono"
                title={$t('common.clickToCopy')}
                use:copyLog={`/api/hooks/${wh.prefix}…`}
              >/api/hooks/{wh.prefix}…</td>
              <td class="num">{wh.received_count}</td>
              <td>{fmtTime(wh.last_received_at)}</td>
              <td>
                <Badge tone={wh.enabled ? 'success' : 'neutral'}>
                  {wh.enabled ? $t('webhooks.enabled') : $t('webhooks.disabled')}
                </Badge>
              </td>
              <td class="actions">
                <button
                  class="icon"
                  title={wh.enabled ? $t('webhooks.disable') : $t('webhooks.enable')}
                  onclick={() => toggleEnabled(wh)}
                >
                  <Icon name={wh.enabled ? 'pause' : 'play'} size={14} />
                </button>
                <button class="icon" title={$t('webhooks.events')} onclick={() => openEvents(wh)}>
                  <Icon name="list" size={14} />
                </button>
                <button class="icon" title={$t('webhooks.edit')} onclick={() => openEdit(wh)}>
                  <Icon name="pencil" size={14} />
                </button>
                <button
                  class="icon danger-hover"
                  title={$t('webhooks.delete')}
                  onclick={() => askDelete(wh)}
                >
                  <Icon name="trash" size={14} />
                </button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}

  <div class="card setup">
    <h3>{$t('webhooks.setupTitle')}</h3>
    <ol class="hint steps">
      <li>{$t('webhooks.setup1')}</li>
      <li>{$t('webhooks.setup2')}</li>
      <li>{$t('webhooks.setup3')}</li>
      <li>{$t('webhooks.setup4')}</li>
    </ol>
  </div>
</div>

<Modal
  bind:open={modalOpen}
  title={editing ? $t('webhooks.editTitle') : $t('webhooks.add')}
  size="sm"
>
  <div class="form">
    <Input
      label={$t('webhooks.name')}
      bind:value={formName}
      placeholder={$t('webhooks.namePlaceholder')}
      maxlength="80"
      required
    />
    <Select label={$t('webhooks.target')} options={targetOptions} bind:value={formTarget} />
    <ErrorText error={formError} />
  </div>
  {#snippet footer()}
    <Button onclick={() => (modalOpen = false)}>{$t('common.cancel')}</Button>
    <Button variant="primary" loading={saving} onclick={save}>
      {editing ? $t('common.save') : $t('webhooks.create')}
    </Button>
  {/snippet}
</Modal>

<Modal
  bind:open={createdOpen}
  title={$t('webhooks.createdTitle')}
  size="md"
  onclose={() => (createdUrl = '')}
>
  <p class="hint">{$t('webhooks.createdBody')}</p>
  <CommandBlock command={createdUrl} />
</Modal>

<Modal bind:open={eventsOpen} title={eventsFor?.name ?? ''} size="lg">
  {#if eventsLoading}
    <Skeleton height="0.85rem" width="60%" />
  {:else if !events.length}
    <EmptyState icon="inbox" compact title={$t('webhooks.noEvents')} />
  {:else}
    <ul class="card-list events-list">
      {#each events as ev (ev.id)}
        <li class="event">
          <div class="event-head">
            <Badge tone={STATUS_TONE[ev.status] ?? 'neutral'}>
              {$t(`webhooks.status.${ev.status}`)}
            </Badge>
            <span class="hint">{fmtTime(ev.received_at)}</span>
            <span class="hint">{ev.detail}</span>
          </div>
          {#if ev.payload}
            <pre class="payload">{ev.payload}</pre>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</Modal>

<ConfirmModal
  bind:open={confirmOpen}
  title={$t('webhooks.delete')}
  message={$t('webhooks.deleteBody', { name: deleting?.name ?? '' })}
  confirmLabel={$t('webhooks.delete')}
  cancelLabel={$t('common.cancel')}
  danger
  onconfirm={confirmDelete}
/>

<style>
  .page {
    height: 100%;
    overflow-y: auto;
    padding: var(--space-6);
  }
  .hint {
    margin: var(--space-2) 0 0;
  }
  /* Advisory banner (same shape as the MCP off-note): amber accent, never blocking. */
  .netwarn {
    display: flex;
    gap: var(--space-2);
    align-items: flex-start;
    border: 1px solid var(--border);
    border-left: 3px solid var(--amber);
    background: var(--surface-2);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    font-size: var(--text-sm);
    color: var(--muted);
    margin-bottom: var(--space-4);
  }
  .netwarn :global(.icon-svg) {
    color: var(--amber-ink);
    margin-top: 2px;
  }
  .netwarn p {
    margin: 0;
  }
  .netwarn p + p {
    margin-top: var(--space-1);
  }
  .netwarn a {
    color: var(--accent);
  }
  .tablewrap {
    overflow-x: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }
  tr.off td {
    opacity: 0.6;
  }
  .strong {
    font-weight: var(--fw-semibold);
  }
  .mono {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .actions {
    text-align: right;
    white-space: nowrap;
  }
  .setup {
    margin-top: var(--space-6);
  }
  .steps {
    margin: var(--space-2) 0 0;
    padding-left: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  /* The delivery log scrolls in place instead of stretching the modal. */
  .events-list {
    max-height: 55vh;
    overflow-y: auto;
    padding-right: var(--space-1);
  }
  /* Event rows hold a payload block under the head line, so they stack. */
  li.event {
    flex-direction: column;
    align-items: stretch;
  }
  .event-head {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .event-head .hint {
    margin: 0;
  }
  .payload {
    margin: var(--space-1) 0 0;
    padding: var(--space-2);
    background: var(--surface-2);
    border-radius: var(--radius);
    font-size: var(--text-xs);
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 160px;
    overflow-y: auto;
  }
</style>
