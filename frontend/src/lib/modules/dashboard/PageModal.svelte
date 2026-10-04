<script>
  // Create or edit a dashboard page: name (the title), description (the subtitle), tag
  // (the switcher chip label) and which modules to place on it. For an existing page the
  // module multi-select reflects what's currently on the page; toggling off removes the
  // tile, toggling on appends one. New pages start from the chosen module set.
  import Modal from '$lib/ui/Modal.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';
  import { visibleModules } from '$lib/modules/registry';
  import { installedIds } from '$lib/modules/installed.js';

  let {
    open = $bindable(false),
    page = null, // existing page to edit, or null to create
    placedIds = [], // moduleIds currently on the page (edit mode)
    onsave = () => {},
    ondelete = null // shown only when editing and >1 page exists
  } = $props();

  const installed = $derived(visibleModules($installedIds).filter((m) => !m.home));

  let name = $state('');
  let description = $state('');
  let tag = $state('');
  let selected = $state(new Set());

  // Re-seed the form whenever the modal opens (for the given page or a fresh create).
  $effect(() => {
    if (!open) return;
    name = page?.name ?? '';
    description = page?.description ?? '';
    tag = page?.tag ?? '';
    selected = new Set(placedIds);
  });

  function toggle(id) {
    const next = new Set(selected);
    next.has(id) ? next.delete(id) : next.add(id);
    selected = next;
  }

  function submit() {
    if (!name.trim()) return;
    onsave({
      name: name.trim(),
      description: description.trim(),
      tag: tag.trim(),
      moduleIds: [...selected]
    });
    open = false;
  }
</script>

<Modal bind:open size="md" title={page ? $t('dashboard.editPage') : $t('dashboard.newPage')}>
  <div class="form">
    <label>
      <span>{$t('dashboard.pageName')}</span>
      <input bind:value={name} placeholder="My dashboard" />
    </label>
    <label>
      <span>{$t('dashboard.pageDesc')}</span>
      <input bind:value={description} placeholder="A short sentence shown under the title." />
    </label>
    <label>
      <span>{$t('dashboard.pageTag')}</span>
      <input bind:value={tag} maxlength="24" placeholder={name || 'Tag'} />
      <small>{$t('dashboard.pageTagHint')}</small>
    </label>

    <fieldset>
      <legend>{$t('dashboard.pageModules')}</legend>
      <div class="mods">
        {#each installed as m}
          <button
            type="button"
            class="mod"
            class:on={selected.has(m.id)}
            onclick={() => toggle(m.id)}
          >
            <span class="mi"><Icon name={m.icon} size={15} /></span>
            <span class="mn">{m.name}</span>
            <span class="chk">{#if selected.has(m.id)}<Icon name="check" size={13} />{/if}</span>
          </button>
        {/each}
      </div>
    </fieldset>
  </div>

  {#snippet footer()}
    {#if ondelete}
      <button class="danger del" onclick={() => { ondelete(); open = false; }}>
        {$t('dashboard.deletePage')}
      </button>
    {/if}
    <button class="ghost" onclick={() => (open = false)}>Cancel</button>
    <button class="primary" onclick={submit} disabled={!name.trim()}>Save</button>
  {/snippet}
</Modal>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  label {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    font-size: var(--text-base);
    color: var(--dim);
  }
  small {
    color: var(--dim);
    font-size: var(--text-xs);
  }
  fieldset {
    border: 0.5px solid var(--border);
    border-radius: 0;
    padding: var(--space-3);
  }
  legend {
    font-size: var(--text-sm);
    color: var(--dim);
    padding: 0 var(--space-2);
  }
  .mods {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
    gap: var(--space-2);
  }
  .mod {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-family: inherit;
    font-size: var(--text-sm);
    text-align: left;
    padding: var(--space-2) var(--space-3);
    background: var(--bg);
    border: 0.5px solid var(--border);
    border-left: 1.5px solid transparent;
    border-radius: 0;
    color: var(--text);
    cursor: pointer;
  }
  .mod:hover {
    background: var(--surface-2);
  }
  .mod.on {
    border-left-color: var(--accent);
    background: var(--surface-2);
  }
  .mod .mn {
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .del {
    margin-right: auto;
  }
  .mi {
    display: inline-flex;
    color: var(--muted);
  }
  .mod.on .mi {
    color: var(--accent);
  }
  .chk {
    display: inline-flex;
    color: var(--accent);
    width: 14px;
  }
</style>
