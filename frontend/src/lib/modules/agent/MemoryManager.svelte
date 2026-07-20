<script>
  // Long-term memory manager. The agent writes small facts here via memory_write (badged
  // "agent"); this pane lets the user browse, add, edit, and delete them — no hidden state.
  // Memories are keyed by slug (upsert replaces on matching slug).
  import { onMount } from 'svelte';
  import { t } from '$lib/i18n';
  import Icon from '$lib/ui/Icon.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Input from '$lib/ui/Input.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import { agentApi } from '$lib/modules/agent/api.js';

  let memories = $state([]);
  let max = $state(200);
  let loading = $state(true);
  let error = $state('');

  let editing = $state(null); // null=closed, {}=new, row=edit
  let form = $state({ slug: '', description: '', content: '', kind: '' });
  let formErr = $state('');
  let saving = $state(false);

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
      const r = await agentApi.listMemories();
      memories = r.memories;
      max = r.max;
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  function openNew() {
    editing = {};
    form = { slug: '', description: '', content: '', kind: 'manual' };
    formErr = '';
  }
  function openEdit(m) {
    editing = m;
    // Keep provenance: editing an agent-written memory doesn't relabel it.
    form = { slug: m.slug, description: m.description, content: m.content, kind: m.kind };
    formErr = '';
  }

  async function save() {
    formErr = '';
    if (!form.slug.trim()) {
      formErr = $t('agent.mem.errSlug');
      return;
    }
    saving = true;
    try {
      await agentApi.upsertMemory(form);
      editing = null;
      await reload();
    } catch (e) {
      formErr = e.message;
    } finally {
      saving = false;
    }
  }

  function remove(m) {
    askConfirm($t('agent.mem.deleteConfirm', { slug: m.slug }), async () => {
      try {
        await agentApi.deleteMemory(m.slug);
        await reload();
      } catch (e) {
        error = e.message;
      }
    });
  }
</script>

<div class="mgr">
  <div class="head">
    <p class="muted">{$t('agent.mem.count', { n: memories.length, max })}</p>
    {#if !editing}
      <Button size="sm" icon="plus" onclick={openNew} disabled={memories.length >= max}>{$t('agent.mem.add')}</Button>
    {/if}
  </div>
  <ErrorText {error} />

  {#if editing}
    <div class="form">
      <Input
        label={$t('agent.mem.slug')}
        placeholder="preferred-currency"
        bind:value={form.slug}
        disabled={!!editing.slug}
        hint={editing.slug ? $t('agent.mem.slugHintEdit') : $t('agent.mem.slugHintNew')}
      />
      <Input label={$t('agent.mem.desc')} placeholder={$t('agent.mem.descPh')} bind:value={form.description} />
      <Input label={$t('agent.mem.content')} multiline rows={5} placeholder={$t('agent.mem.contentPh')} bind:value={form.content} />
      <ErrorText error={formErr} />
      <div class="row">
        <Button variant="primary" loading={saving} onclick={save}>{$t('common.save')}</Button>
        <Button variant="ghost" onclick={() => (editing = null)}>{$t('common.cancel')}</Button>
      </div>
    </div>
  {:else if loading}
    <p class="muted">{$t('common.loading')}</p>
  {:else if !memories.length}
    <EmptyState icon="file-text" title={$t('agent.mem.empty')} description={$t('agent.mem.emptyDesc')} />
  {:else}
    <ul class="list">
      {#each memories as m (m.id)}
        <li>
          <div class="main">
            <div class="slug-row">
              <code>{m.slug}</code>
              {#if m.kind === 'agent'}<Badge tone="accent">{$t('agent.mem.byAgent')}</Badge>{/if}
            </div>
            {#if m.description}<span class="desc">{m.description}</span>{/if}
          </div>
          <div class="actions">
            <button class="icon" title={$t('agent.edit')} onclick={() => openEdit(m)}><Icon name="pencil" size={14} /></button>
            <button class="icon danger" title={$t('agent.delete')} onclick={() => remove(m)}><Icon name="trash" size={14} /></button>
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<ConfirmModal
  bind:open={confirmOpen}
  title={$t('agent.confirm.title')}
  message={confirmMessage}
  confirmLabel={$t('common.delete')}
  danger
  onconfirm={onConfirmYes}
/>

<style>
  .mgr {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-3);
    background: var(--surface-2);
  }
  .list {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .list li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    background: var(--surface);
  }
  .main {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .slug-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .main code {
    font-family: var(--mono);
    color: var(--text);
  }
  .desc {
    color: var(--muted);
    font-size: var(--text-xs);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .actions {
    display: flex;
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
  .row {
    display: flex;
    gap: var(--space-2);
  }
  .muted {
    color: var(--muted);
    font-size: var(--text-sm);
  }
</style>
