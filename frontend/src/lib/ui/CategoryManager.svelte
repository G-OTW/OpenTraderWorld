<script>
  // Manage the categories that group templates: rename, recolour, reorder, add, remove.
  //
  // Every row edits in place and saves on blur — a category is a name and a colour, so a
  // per-row edit mode would be more ceremony than the thing being edited. Deleting one
  // never deletes templates: they fall back to "uncategorised" (ON DELETE SET NULL), which
  // is why removal is a plain button and not a scary confirm.
  import Modal from '$lib/ui/Modal.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { CATEGORY_COLORS } from '$lib/ui/consistency.js';
  import { t } from '$lib/i18n';

  // `api` is injected ({ createCategory, updateCategory, deleteCategory }) so the same
  // manager serves any module that groups things by category.
  let { open = $bindable(false), categories = [], api, onsaved = () => {} } = $props();

  let rows = $state([]);
  let newName = $state('');
  let newColor = $state(CATEGORY_COLORS[0]);
  let error = $state('');
  let busy = $state(false);

  // Local working copy: the modal edits its own list and calls onsaved() so the page
  // reloads from the server, rather than mutating the prop it was handed.
  $effect(() => {
    if (open) rows = categories.map((c) => ({ ...c }));
  });

  async function run(fn) {
    busy = true;
    error = '';
    try {
      await fn();
      onsaved();
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }

  async function rename(row) {
    const name = row.name.trim();
    const original = categories.find((c) => c.id === row.id);
    if (!name || name === original?.name) return;
    await run(() => api.updateCategory(row.id, { name }));
  }

  async function recolor(row, color) {
    row.color = color;
    await run(() => api.updateCategory(row.id, { color }));
  }

  async function move(idx, delta) {
    const to = idx + delta;
    if (to < 0 || to >= rows.length) return;
    const next = [...rows];
    [next[idx], next[to]] = [next[to], next[idx]];
    rows = next;
    // Positions are rewritten from the new array order, so they stay dense and gap-free.
    await run(async () => {
      for (const [i, c] of next.entries()) {
        await api.updateCategory(c.id, { position: i });
      }
    });
  }

  async function add() {
    const name = newName.trim();
    if (!name) return;
    await run(async () => {
      await api.createCategory(name, newColor);
      newName = '';
      // Step the default colour along so consecutive additions don't all look alike.
      newColor = CATEGORY_COLORS[(CATEGORY_COLORS.indexOf(newColor) + 1) % CATEGORY_COLORS.length];
    });
  }

  async function remove(row) {
    await run(() => api.deleteCategory(row.id));
  }
</script>

<Modal bind:open size="md" title={$t('routines.categories.title')}>
  <div class="wrap">
    <p class="intro">{$t('routines.categories.intro')}</p>

    <ul class="list">
      {#each rows as row, i (row.id)}
        <li class="row">
          <div class="swatches" role="group" aria-label={$t('routines.categories.colour')}>
            {#each CATEGORY_COLORS as c (c)}
              <button
                type="button"
                class="sw"
                class:on={row.color === c}
                style:background={c}
                aria-label={$t('routines.categories.colour')}
                aria-pressed={row.color === c}
                onclick={() => recolor(row, c)}
              ></button>
            {/each}
          </div>
          <input
            class="name"
            bind:value={row.name}
            onblur={() => rename(row)}
            onkeydown={(e) => e.key === 'Enter' && e.currentTarget.blur()}
            aria-label={$t('routines.categories.name')}
          />
          <button class="icon" onclick={() => move(i, -1)} disabled={i === 0} title={$t('routines.editor.moveUp')} aria-label={$t('routines.editor.moveUp')}>
            <Icon name="arrow-up" size={13} />
          </button>
          <button class="icon" onclick={() => move(i, 1)} disabled={i === rows.length - 1} title={$t('routines.editor.moveDown')} aria-label={$t('routines.editor.moveDown')}>
            <Icon name="arrow-down" size={13} />
          </button>
          <button class="icon del" onclick={() => remove(row)} title={$t('common.delete')} aria-label={$t('common.delete')}>
            <Icon name="trash" size={13} />
          </button>
        </li>
      {/each}
    </ul>

    {#if rows.length === 0}
      <p class="hint">{$t('routines.categories.empty')}</p>
    {/if}

    <div class="addrow">
      <div class="swatches" role="group" aria-label={$t('routines.categories.colour')}>
        {#each CATEGORY_COLORS as c (c)}
          <button
            type="button"
            class="sw"
            class:on={newColor === c}
            style:background={c}
            aria-label={$t('routines.categories.colour')}
            aria-pressed={newColor === c}
            onclick={() => (newColor = c)}
          ></button>
        {/each}
      </div>
      <input
        class="name"
        placeholder={$t('routines.categories.newPlaceholder')}
        bind:value={newName}
        onkeydown={(e) => e.key === 'Enter' && add()}
      />
      <button class="btn" onclick={add} disabled={busy || !newName.trim()}>
        <Icon name="plus" size={13} /> {$t('routines.categories.add')}
      </button>
    </div>

    <p class="note">{$t('routines.categories.deleteNote')}</p>

    <ErrorText error={error} />

    <div class="foot">
      <div class="spacer"></div>
      <button class="btn primary" onclick={() => (open = false)}>{$t('common.done')}</button>
    </div>
  </div>
</Modal>

<style>
  .wrap {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .intro,
  .note,
  .hint {
    margin: 0;
    color: var(--muted);
    font-size: var(--text-xs);
  }

  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .row,
  .addrow {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .addrow {
    border-top: 0.5px solid var(--border);
    padding-top: var(--space-3);
  }
  .name {
    flex: 1;
    min-width: 0;
  }

  .swatches {
    display: flex;
    gap: 2px;
    flex: none;
  }
  .sw {
    width: 13px;
    height: 13px;
    border-radius: 50%;
    border: 1px solid transparent;
    cursor: pointer;
    padding: 0;
  }
  .sw.on {
    /* The ring sits outside the dot so the colour itself is never obscured. */
    outline: 1px solid var(--text);
    outline-offset: 1px;
  }

  .icon {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: 3px;
    border-radius: var(--radius);
    display: inline-flex;
    flex: none;
  }
  .icon:hover:not(:disabled) {
    color: var(--text);
    background: var(--surface-2);
  }
  .icon:disabled {
    opacity: 0.35;
    cursor: default;
  }
  .icon.del:hover {
    color: var(--red);
  }

  .foot {
    display: flex;
    gap: var(--space-2);
  }
  .spacer {
    flex: 1;
  }
  .btn {
    background: var(--surface-2);
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-3);
    color: var(--text);
    font-size: var(--text-base);
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
  }
  .btn.primary {
    border-color: var(--border-control);
    font-weight: var(--fw-medium);
  }
  .btn:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
