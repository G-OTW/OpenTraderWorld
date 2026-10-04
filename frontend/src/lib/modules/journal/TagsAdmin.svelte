<script>
  // Discipline tags. Three kinds, one purpose: make the trading plan measurable.
  //
  //   mistake — a rule broken. The analytics screen prices these against the trades
  //             logged without any breach, so "I keep chasing" gets a number.
  //   rule    — a rule honoured. Adherence over time, per rule.
  //   setup   — neutral classification, for slicing stats by anything else.
  //
  // The kind never changes the math, only how the stat reads.
  import { journalApi, TAG_KINDS } from './api.js';
  import Button from '$lib/ui/Button.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';

  let { tags = [], ontagsChanged = () => {} } = $props();

  const KIND_TONE = { mistake: 'danger', rule: 'success', setup: 'neutral' };

  let name = $state('');
  let kind = $state('mistake');
  let description = $state('');
  let error = $state('');
  let saving = $state(false);

  // Inline rename: one row at a time, Enter commits, Escape drops the edit.
  let editingId = $state('');
  let editName = $state('');

  let confirmOpen = $state(false);
  let confirmMessage = $state('');
  let onConfirmYes = $state(() => {});

  async function add() {
    const n = name.trim();
    if (!n || saving) return;
    saving = true;
    error = '';
    try {
      await journalApi.addTag({ name: n, kind, description: description.trim() || null });
      name = '';
      description = '';
      ontagsChanged();
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }

  function startEdit(tag) {
    editingId = tag.id;
    editName = tag.name;
  }

  async function commitEdit(tag) {
    const n = editName.trim();
    editingId = '';
    if (!n || n === tag.name) return;
    try {
      await journalApi.updateTag(tag.id, { name: n });
      ontagsChanged();
    } catch (e) {
      error = e.message;
    }
  }

  async function setKind(tag, next) {
    if (next === tag.kind) return;
    await journalApi.updateTag(tag.id, { kind: next });
    ontagsChanged();
  }

  function del(tag) {
    confirmMessage = $t('journal.tags.confirmDelete', { name: tag.name, count: tag.trade_count });
    onConfirmYes = async () => {
      await journalApi.deleteTag(tag.id);
      ontagsChanged();
    };
    confirmOpen = true;
  }

  const byKind = $derived(
    TAG_KINDS.map((k) => ({ kind: k, items: tags.filter((tag) => tag.kind === k) }))
  );
</script>

<div class="tags">
  <section class="card">
    <h3>{$t('journal.tags.title')}</h3>
    <p class="hint">{$t('journal.tags.hint')}</p>

    <div class="add-row">
      <input
        placeholder={$t('journal.tags.namePlaceholder')}
        bind:value={name}
        onkeydown={(e) => e.key === 'Enter' && add()}
      />
      <Dropdown
        bind:value={kind}
        title={$t('journal.tags.kindLabel')}
        ariaLabel={$t('journal.tags.kindLabel')}
        options={TAG_KINDS.map((k) => ({ value: k, label: $t(`journal.tags.kind.${k}`) }))}
      />
      <input placeholder={$t('journal.tags.descriptionPlaceholder')} bind:value={description} />
      <Button variant="primary" icon="plus" onclick={add} loading={saving}>
        {$t('journal.tags.add')}
      </Button>
    </div>
    {#if error}<ErrorText>{error}</ErrorText>{/if}

    {#if tags.length === 0}
      <EmptyState
        icon="tag"
        title={$t('journal.tags.empty.title')}
        description={$t('journal.tags.empty.description')}
        compact
      />
    {:else}
      {#each byKind as group (group.kind)}
        {#if group.items.length}
          <h4>
            <Badge tone={KIND_TONE[group.kind]}>{$t(`journal.tags.kind.${group.kind}`)}</Badge>
            <span class="kind-hint">{$t(`journal.tags.kindHint.${group.kind}`)}</span>
          </h4>
          <ul class="card-list">
            {#each group.items as tag (tag.id)}
              <li>
                <div class="main">
                  {#if editingId === tag.id}
                    <!-- svelte-ignore a11y_autofocus -->
                    <input
                      class="rename"
                      autofocus
                      bind:value={editName}
                      onblur={() => commitEdit(tag)}
                      onkeydown={(e) => {
                        if (e.key === 'Enter') commitEdit(tag);
                        if (e.key === 'Escape') editingId = '';
                      }}
                    />
                  {:else}
                    <button class="rename-btn" onclick={() => startEdit(tag)}>{tag.name}</button>
                  {/if}
                  {#if tag.description}<span class="desc">{tag.description}</span>{/if}
                </div>
                <span class="count">{$t('journal.tags.tradeCount', { count: tag.trade_count })}</span>
                <Dropdown
                  value={tag.kind}
                  ariaLabel={$t('journal.tags.kindLabel')}
                  options={TAG_KINDS.map((k) => ({ value: k, label: $t(`journal.tags.kind.${k}`) }))}
                  onpick={(v) => setKind(tag, v)}
                />
                <button
                  class="icon danger-hover"
                  onclick={() => del(tag)}
                  aria-label={$t('common.delete')}
                  title={$t('common.delete')}
                >
                  <Icon name="trash" size={14} />
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      {/each}
    {/if}
  </section>
</div>

<ConfirmModal
  bind:open={confirmOpen}
  title={$t('journal.tags.confirmTitle')}
  message={confirmMessage}
  confirmLabel={$t('journal.fees.deleteModal.confirm')}
  cancelLabel={$t('common.cancel')}
  danger
  onconfirm={() => onConfirmYes()}
/>

<style>
  .tags {
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
  }
  .card {
    background: var(--surface);
    border: 0.5px solid var(--border);
    border-radius: 0;
    padding: var(--space-4);
  }
  .card h3 {
    font-size: 12.5px;
    font-weight: var(--fw-medium);
    letter-spacing: 0.03em;
  }
  .hint {
    color: var(--dim);
    font-size: var(--text-xs);
    margin: var(--space-2) 0 var(--space-4);
    line-height: var(--lh-base);
  }
  .add-row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    margin-bottom: var(--space-4);
  }
  .add-row input {
    flex: 1 1 160px;
    min-width: 0;
  }
  h4 {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: var(--space-4) 0 var(--space-2);
    font-weight: var(--fw-normal);
  }
  .kind-hint {
    font-size: var(--text-xs);
    color: var(--dim);
  }
  .main {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    min-width: 0;
    flex: 1;
  }
  .rename-btn {
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    color: var(--text);
    cursor: text;
    text-align: left;
  }
  .rename-btn:hover {
    text-decoration: underline dotted;
  }
  .rename {
    max-width: 220px;
  }
  .desc {
    font-size: var(--text-xs);
    color: var(--dim);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .count {
    font-size: var(--text-xs);
    color: var(--faint);
    font-family: var(--mono);
    white-space: nowrap;
  }
</style>
