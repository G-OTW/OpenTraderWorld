<script>
  // Skills manager — the one place a skill is created or edited.
  //
  // A skill is SKILL.md-like: name + description (always in the assistant's context) + a body
  // loaded on demand via load_skill. There is ONE catalog: personas pick from it, they do not
  // own copies. So every row carries how many personas hold it — an edit here lands in all of
  // them, and a delete takes it off all their shelves.
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

  let rows = $state([]); // [{ skill, used_by }]
  let warnChars = $state(8000);
  let loading = $state(true);
  let error = $state('');
  let importing = $state(false);
  let importMsg = $state('');
  let fileInput;

  let editing = $state(null);
  let form = $state({ name: '', description: '', body: '', enabled: true });
  let formErr = $state('');
  let saving = $state(false);

  // A long body is a design smell, not an error: a loaded skill lands whole in the context
  // window, so it competes with the task for room. Say so while it is being written.
  const tooLong = $derived(form.body.length > warnChars);

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
      const res = await agentApi.listSkills();
      rows = res.skills;
      warnChars = res.body_warn_chars ?? warnChars;
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  /** Import a bundle exported from here or from the personas tab. */
  async function onFile(ev) {
    const file = ev.target.files?.[0];
    ev.target.value = '';
    if (!file) return;
    importing = true;
    importMsg = '';
    error = '';
    try {
      const bundle = JSON.parse(await file.text());
      const r = await agentApi.importShelf(bundle);
      importMsg = $t('agent.shelf.imported', {
        added: r.skills_added,
        updated: r.skills_updated,
        skipped: r.skills_skipped
      });
      await reload();
    } catch (e) {
      error = e.message;
    } finally {
      importing = false;
    }
  }

  function openNew() {
    editing = {};
    form = { name: '', description: '', body: '', enabled: true };
    formErr = '';
  }
  function openEdit(s, usedBy = 0) {
    editing = { ...s, usedBy };
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

  function remove(row) {
    // Name the blast radius: deleting takes the skill off every shelf that holds it.
    const msg =
      row.used_by > 0
        ? $t('agent.skl.deleteConfirmUsed', { name: row.skill.name, n: row.used_by })
        : $t('agent.skl.deleteConfirm', { name: row.skill.name });
    askConfirm(msg, async () => {
      try {
        await agentApi.deleteSkill(row.skill.id);
        await reload();
      } catch (e) {
        error = e.message;
      }
    });
  }
</script>

<div class="mgr">
  <div class="head">
    <p class="muted">{$t('agent.skl.count', { n: rows.length, s: rows.length === 1 ? '' : 's' })}</p>
    {#if !editing}
      <div class="row">
        <Button size="sm" variant="ghost" icon="upload" loading={importing} onclick={() => fileInput?.click()}>
          {$t('agent.shelf.import')}
        </Button>
        <Button size="sm" icon="plus" onclick={openNew}>{$t('agent.skl.add')}</Button>
      </div>
    {/if}
  </div>
  <input bind:this={fileInput} type="file" accept="application/json,.json" hidden onchange={onFile} />
  <ErrorText {error} />
  {#if importMsg}<p class="ok">{importMsg}</p>{/if}

  {#if editing}
    <div class="form">
      <Input label={$t('agent.skl.name')} placeholder="e.g. money-management" bind:value={form.name} />
      <Input label={$t('agent.skl.desc')} placeholder={$t('agent.skl.descPh')} bind:value={form.description} />
      <Input label={$t('agent.skl.body')} multiline rows={8} placeholder={$t('agent.skl.bodyPh')} bind:value={form.body} />
      {#if tooLong}
        <p class="warn">{$t('agent.skl.tooLong', { n: form.body.length, max: warnChars })}</p>
      {/if}
      {#if editing.usedBy > 1}
        <p class="warn">{$t('agent.skl.editShared', { n: editing.usedBy })}</p>
      {/if}
      <label class="check"><input type="checkbox" bind:checked={form.enabled} /> {$t('agent.skl.enabled')}</label>
      <ErrorText error={formErr} />
      <div class="row">
        <Button variant="primary" loading={saving} onclick={save}>{$t('common.save')}</Button>
        <Button variant="ghost" onclick={() => (editing = null)}>{$t('common.cancel')}</Button>
      </div>
    </div>
  {:else if loading}
    <p class="muted">{$t('common.loading')}</p>
  {:else if !rows.length}
    <EmptyState icon="book-open" title={$t('agent.skl.empty')} description={$t('agent.skl.emptyDesc')} />
  {:else}
    <ul class="list">
      {#each rows as r (r.skill.id)}
        <li class:disabled={!r.skill.enabled}>
          <div class="main">
            <div class="name-row">
              <strong>{r.skill.name}</strong>
              {#if r.skill.builtin}<Badge tone="accent">{$t('agent.skl.builtin')}</Badge>{/if}
              {#if !r.skill.enabled}<Badge tone="neutral">{$t('agent.skl.disabled')}</Badge>{/if}
              <span class="uses">{$t('agent.skl.usedBy', { n: r.used_by })}</span>
            </div>
            {#if r.skill.description}<span class="desc">{r.skill.description}</span>{/if}
          </div>
          <div class="actions">
            <a class="icon" title={$t('agent.shelf.export')} href={agentApi.shelfExportUrl({ skill: r.skill.id })} download>
              <Icon name="download" size={14} />
            </a>
            <button class="icon" title={r.skill.enabled ? $t('agent.skl.disable') : $t('agent.skl.enable')} onclick={() => toggle(r.skill)}>
              <Icon name={r.skill.enabled ? 'check-circle' : 'eye-off'} size={14} />
            </button>
            <button class="icon" title={$t('agent.edit')} onclick={() => openEdit(r.skill, r.used_by)}><Icon name="pencil" size={14} /></button>
            <button class="icon danger" title={$t('agent.delete')} onclick={() => remove(r)}><Icon name="trash" size={14} /></button>
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
  .uses {
    color: var(--muted);
    font-size: var(--text-xs);
    white-space: nowrap;
  }
  .warn {
    color: var(--amber);
    font-size: var(--text-xs);
  }
  .ok {
    color: var(--green);
    font-size: var(--text-sm);
  }
  a.icon {
    text-decoration: none;
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
