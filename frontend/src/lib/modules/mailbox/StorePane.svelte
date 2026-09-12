<script>
  // The Store: the newsletters you picked, as cards you can open in one click.
  // Grouped by domain, filtered by topic, searchable. It stands on its own — no
  // mailbox needs to be connected for the list to be useful.
  import Icon from '$lib/ui/Icon.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Input from '$lib/ui/Input.svelte';
  import Select from '$lib/ui/Select.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import { mailboxApi, TOPICS } from './api.js';
  import { t } from '$lib/i18n';

  let links = $state([]);
  let loading = $state(true);
  let error = $state('');

  let topic = $state('all');
  let search = $state('');

  let formOpen = $state(false);
  let editing = $state(null);
  let form = $state({ name: '', url: '', description: '', topic: 'trading', subscribed: false });
  let formError = $state('');
  let saving = $state(false);
  let confirmDelete = $state(null);

  $effect(() => {
    load();
  });

  async function load() {
    loading = true;
    error = '';
    try {
      links = await mailboxApi.listLinks();
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  const topicOptions = $derived(TOPICS.map((v) => ({ value: v, label: $t(`mailbox.topic.${v}`) })));

  const visible = $derived.by(() => {
    const q = search.trim().toLowerCase();
    return links.filter(
      (l) =>
        (topic === 'all' || l.topic === topic) &&
        (!q ||
          l.name.toLowerCase().includes(q) ||
          l.domain.includes(q) ||
          l.description.toLowerCase().includes(q))
    );
  });

  const grouped = $derived.by(() => {
    const map = new Map();
    for (const l of visible) {
      const key = l.domain || $t('mailbox.noDomain');
      if (!map.has(key)) map.set(key, []);
      map.get(key).push(l);
    }
    return [...map.entries()].sort((a, b) => a[0].localeCompare(b[0]));
  });

  function countFor(v) {
    return v === 'all' ? links.length : links.filter((l) => l.topic === v).length;
  }

  function openCreate() {
    editing = null;
    form = { name: '', url: '', description: '', topic: 'trading', subscribed: false };
    formError = '';
    formOpen = true;
  }

  function openEdit(l) {
    editing = l;
    form = {
      name: l.name,
      url: l.url,
      description: l.description,
      topic: l.topic,
      subscribed: l.subscribed
    };
    formError = '';
    formOpen = true;
  }

  async function save() {
    if (!form.name.trim()) {
      formError = $t('mailbox.err.nameRequired');
      return;
    }
    saving = true;
    formError = '';
    try {
      if (editing) await mailboxApi.updateLink(editing.id, form);
      else await mailboxApi.createLink(form);
      formOpen = false;
      await load();
    } catch (e) {
      formError = e.message;
    } finally {
      saving = false;
    }
  }

  async function toggleSubscribed(l) {
    await mailboxApi.updateLink(l.id, { subscribed: !l.subscribed });
    l.subscribed = !l.subscribed;
  }

  async function remove() {
    const l = confirmDelete;
    confirmDelete = null;
    if (!l) return;
    await mailboxApi.deleteLink(l.id);
    await load();
  }
</script>

<div class="bar">
  <div class="topics">
    {#each ['all', ...TOPICS] as v (v)}
      <button class:active={topic === v} onclick={() => (topic = v)}>
        {v === 'all' ? $t('mailbox.topic.all') : $t(`mailbox.topic.${v}`)}
        <span class="n">{countFor(v)}</span>
      </button>
    {/each}
  </div>
  <div class="right">
    <div class="searchwrap">
      <Icon name="search" size={13} />
      <input type="search" placeholder={$t('mailbox.searchStore')} bind:value={search} />
    </div>
    <Button variant="primary" icon="plus" onclick={openCreate}>{$t('mailbox.addLink')}</Button>
  </div>
</div>

{#if error}<ErrorText {error} />{/if}

{#if loading}
  <Skeleton rows={5} />
{:else if !links.length}
  <EmptyState
    icon="newspaper"
    title={$t('mailbox.storeEmptyTitle')}
    description={$t('mailbox.storeEmptyDesc')}
  >
    {#snippet action()}
      <Button variant="primary" icon="plus" onclick={openCreate}>{$t('mailbox.addLink')}</Button>
    {/snippet}
  </EmptyState>
{:else if !visible.length}
  <EmptyState compact icon="search" title={$t('mailbox.noMatch')} />
{:else}
  {#each grouped as [domain, list] (domain)}
    <section class="group">
      <h3>{domain}<span class="count">{list.length}</span></h3>
      <div class="grid">
        {#each list as l (l.id)}
          <article class="card">
            <header>
              <h4>{l.name}</h4>
              <span class="topic">{$t(`mailbox.topic.${l.topic}`)}</span>
            </header>
            {#if l.description}<p class="desc">{l.description}</p>{/if}
            <footer>
              {#if l.url}
                <a class="open" href={l.url} target="_blank" rel="noopener noreferrer">
                  <Icon name="external-link" size={12} />
                  {$t('mailbox.open')}
                </a>
              {/if}
              <button
                class="sub"
                class:on={l.subscribed}
                onclick={() => toggleSubscribed(l)}
                title={$t('mailbox.subscribedHint')}
              >
                <Icon name={l.subscribed ? 'check-circle' : 'plus'} size={12} />
                {l.subscribed ? $t('mailbox.subscribed') : $t('mailbox.notSubscribed')}
              </button>
              <span class="spacer"></span>
              <button class="icon" onclick={() => openEdit(l)} aria-label={$t('common.edit')}>
                <Icon name="pencil" size={13} />
              </button>
              <button
                class="icon danger"
                onclick={() => (confirmDelete = l)}
                aria-label={$t('common.delete')}
              >
                <Icon name="trash" size={13} />
              </button>
            </footer>
          </article>
        {/each}
      </div>
    </section>
  {/each}
{/if}

<Modal
  bind:open={formOpen}
  size="md"
  title={editing ? $t('mailbox.editLink') : $t('mailbox.addLink')}
>
  <div class="form">
    <Input label={$t('mailbox.linkName')} bind:value={form.name} required />
    <Input
      label={$t('mailbox.linkUrl')}
      bind:value={form.url}
      placeholder="https://…"
      hint={$t('mailbox.linkUrlHint')}
    />
    <Input
      label={$t('mailbox.linkDesc')}
      bind:value={form.description}
      multiline
      rows={3}
      hint={$t('mailbox.linkDescHint')}
    />
    <Select label={$t('mailbox.topicLabel')} options={topicOptions} bind:value={form.topic} />
    <label class="check">
      <input type="checkbox" bind:checked={form.subscribed} />
      <span>{$t('mailbox.subscribedLabel')}</span>
    </label>
    {#if formError}<ErrorText error={formError} />{/if}
  </div>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (formOpen = false)}>{$t('common.cancel')}</Button>
    <Button variant="primary" loading={saving} onclick={save}>{$t('common.save')}</Button>
  {/snippet}
</Modal>

<ConfirmModal
  open={!!confirmDelete}
  title={$t('mailbox.deleteLink')}
  message={$t('mailbox.deleteLinkMsg', { name: confirmDelete?.name })}
  confirmLabel={$t('common.delete')}
  danger
  onconfirm={remove}
  oncancel={() => (confirmDelete = null)}
/>

<style>
  .bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    flex-wrap: wrap;
    margin-bottom: var(--space-4);
  }
  .topics {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
  }
  .topics button {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    border: 1px solid transparent;
    background: var(--surface-2);
    color: var(--muted);
    border-radius: 999px;
    padding: 3px var(--space-3);
    font-size: var(--text-xs);
    cursor: pointer;
  }
  .topics button.active {
    background: var(--surface);
    border-color: var(--border);
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  .topics .n {
    color: var(--muted);
    font-size: 10px;
  }

  .right {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .searchwrap {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: 0 var(--space-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--muted);
  }
  .searchwrap input {
    border: 0;
    background: none;
    padding: 5px 0;
    font-size: var(--text-sm);
    color: var(--text);
    outline: none;
    width: 180px;
    max-width: 40vw;
  }

  .group {
    margin-bottom: var(--space-6);
  }
  .group h3 {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: 0 0 var(--space-2);
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--muted);
  }
  .count {
    font-size: 10px;
    color: var(--muted);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 0 6px;
    text-transform: none;
    letter-spacing: 0;
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
    gap: var(--space-3);
  }
  .card {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    padding: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .card header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .card h4 {
    margin: 0;
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    color: var(--text);
    overflow-wrap: anywhere;
  }
  .topic {
    flex: none;
    font-size: 10px;
    color: var(--muted);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 0 6px;
    line-height: 16px;
  }
  .desc {
    margin: 0;
    font-size: var(--text-xs);
    line-height: 1.5;
    color: var(--muted);
  }
  .card footer {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-top: auto;
  }
  .open {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: var(--text-xs);
    color: var(--text);
    text-decoration: none;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 2px 8px;
  }
  .open:hover {
    border-color: var(--accent);
  }
  .sub {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: none;
    border: 0;
    color: var(--muted);
    font-size: var(--text-xs);
    cursor: pointer;
    padding: 2px;
  }
  .sub.on {
    color: var(--green);
  }
  .spacer {
    flex: 1;
  }
  .icon {
    background: none;
    border: 0;
    color: var(--muted);
    cursor: pointer;
    padding: 2px;
    border-radius: var(--radius);
  }
  .icon:hover {
    color: var(--text);
  }
  .icon.danger:hover {
    color: var(--red);
  }

  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .check {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
    color: var(--text);
  }
</style>
