<script>
  // Skills manager. A skill is SKILL.md-like: name + description (always in the assistant's
  // context) + a body loaded on demand via load_skill. Enable/disable, edit, delete.
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

  let skills = $state([]);
  let loading = $state(true);
  let error = $state('');

  let editing = $state(null);
  let form = $state({ name: '', description: '', body: '', enabled: true });
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
      skills = await agentApi.listSkills();
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  function openNew() {
    editing = {};
    form = { name: '', description: '', body: '', enabled: true };
    formErr = '';
  }
  function openEdit(s) {
    editing = s;
    form = { name: s.name, description: s.description, body: s.body, enabled: s.enabled };
    formErr = '';
  }

  async function save() {
    formErr = '';
    if (!form.name.trim()) {
      formErr = $t('agent.skl.errName');
      return;
    }
    saving = true;
    try {
      if (editing.id) await agentApi.updateSkill(editing.id, form);
      else await agentApi.addSkill(form);
      editing = null;
      await reload();
    } catch (e) {
      formErr = e.message;
    } finally {
      saving = false;
    }
  }

  async function toggle(s) {
    try {
      await agentApi.updateSkill(s.id, { ...s, enabled: !s.enabled });
      await reload();
    } catch (e) {
      error = e.message;
    }
  }

  function remove(s) {
    askConfirm($t('agent.skl.deleteConfirm', { name: s.name }), async () => {
      try {
        await agentApi.deleteSkill(s.id);
        await reload();
      } catch (e) {
        error = e.message;
      }
    });
  }
</script>

<div class="mgr">
  <div class="head">
    <p class="muted">{$t('agent.skl.count', { n: skills.length, s: skills.length === 1 ? '' : 's' })}</p>
    {#if !editing}
      <Button size="sm" icon="plus" onclick={openNew}>{$t('agent.skl.add')}</Button>
    {/if}
  </div>
  <ErrorText {error} />

  {#if editing}
    <div class="form">
      <Input label={$t('agent.skl.name')} placeholder="e.g. money-management" bind:value={form.name} />
      <Input label={$t('agent.skl.desc')} placeholder={$t('agent.skl.descPh')} bind:value={form.description} />
      <Input label={$t('agent.skl.body')} multiline rows={8} placeholder={$t('agent.skl.bodyPh')} bind:value={form.body} />
      <label class="check"><input type="checkbox" bind:checked={form.enabled} /> {$t('agent.skl.enabled')}</label>
      <ErrorText error={formErr} />
      <div class="row">
        <Button variant="primary" loading={saving} onclick={save}>{$t('common.save')}</Button>
        <Button variant="ghost" onclick={() => (editing = null)}>{$t('common.cancel')}</Button>
      </div>
    </div>
  {:else if loading}
    <p class="muted">{$t('common.loading')}</p>
  {:else if !skills.length}
    <EmptyState icon="book-open" title={$t('agent.skl.empty')} description={$t('agent.skl.emptyDesc')} />
  {:else}
    <ul class="list">
      {#each skills as s (s.id)}
        <li class:disabled={!s.enabled}>
          <div class="main">
            <div class="name-row">
              <strong>{s.name}</strong>
              {#if s.builtin}<Badge tone="accent">{$t('agent.skl.builtin')}</Badge>{/if}
              {#if !s.enabled}<Badge tone="neutral">{$t('agent.skl.disabled')}</Badge>{/if}
            </div>
            {#if s.description}<span class="desc">{s.description}</span>{/if}
          </div>
          <div class="actions">
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
  .list li.disabled {
    opacity: 0.65;
  }
  .main {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .name-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .name-row strong {
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
  .check {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: var(--text-sm);
    color: var(--muted);
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
