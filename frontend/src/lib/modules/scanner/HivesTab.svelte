<script>
  // Hives: named groups of items, each with an assigned color (a palette slot, so it follows
  // the theme). A hive is what an alert scans and what a board view shows. The cap on items
  // per hive is a setting; a membership over it is refused with the count named.
  import Button from '$lib/ui/Button.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Input from '$lib/ui/Input.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import ColorSwatches from './ColorSwatches.svelte';
  import { scannerApi } from './api.js';
  import { cssColor, nextSlot } from './board.js';
  import { t } from '$lib/i18n';

  let { items = [], hives = [], cap = 200, onchanged } = $props();

  let error = $state('');
  let editing = $state(null); // { id?, name, color }
  let members = $state(null); // { hive, picked: {id: bool}, q }
  let deleting = $state(null);
  let capDraft = $state('');
  let capOpen = $state(false);

  const itemById = $derived(new Map(items.map((i) => [i.id, i])));

  function create() {
    editing = { id: null, name: '', color: nextSlot(hives.map((h) => h.color)) };
  }

  async function save() {
    try {
      if (editing.id) await scannerApi.saveHive(editing.id, { name: editing.name, color: editing.color });
      else await scannerApi.createHive({ name: editing.name, color: editing.color });
      editing = null;
      error = '';
      onchanged?.();
    } catch (e) {
      error = e.message;
    }
  }

  function openMembers(h) {
    const picked = {};
    for (const id of h.item_ids) picked[id] = true;
    members = { hive: h, picked, q: '' };
  }

  const memberCount = $derived(members ? Object.values(members.picked).filter(Boolean).length : 0);
  const memberRows = $derived(
    members
      ? items.filter((i) => {
          const f = members.q.trim().toLowerCase();
          return !f || i.ticker.toLowerCase().includes(f) || (i.label ?? '').toLowerCase().includes(f);
        })
      : []
  );

  async function saveMembers() {
    const ids = Object.entries(members.picked).filter(([, v]) => v).map(([k]) => k);
    try {
      await scannerApi.setHiveItems(members.hive.id, ids);
      members = null;
      error = '';
      onchanged?.();
    } catch (e) {
      error = e.message;
    }
  }

  async function remove() {
    try {
      await scannerApi.deleteHive(deleting.id);
      deleting = null;
      onchanged?.();
    } catch (e) {
      error = e.message;
    }
  }

  async function saveCap() {
    try {
      await scannerApi.saveSettings({ hive_cap: Number(capDraft) });
      capOpen = false;
      error = '';
      onchanged?.();
    } catch (e) {
      error = e.message;
    }
  }
</script>

<div class="wrap">
  <div class="filter-bar">
    <Button variant="primary" icon="plus" onclick={create}>{$t('scanner.hives.new')}</Button>
    <span class="cap">{$t('scanner.hives.cap', { cap })}</span>
    <Button
      size="sm"
      icon="settings"
      onclick={() => {
        capDraft = String(cap);
        capOpen = true;
      }}>{$t('scanner.hives.changeCap')}</Button
    >
  </div>
  {#if error}<ErrorText {error} />{/if}

  {#if !hives.length}
    <EmptyState icon="layers" title={$t('scanner.hives.emptyTitle')} description={$t('scanner.hives.emptyBody')}>
      {#snippet action()}
        <Button variant="primary" icon="plus" onclick={create}>{$t('scanner.hives.new')}</Button>
      {/snippet}
    </EmptyState>
  {:else}
    <div class="grid">
      {#each hives as h (h.id)}
        <section class="card hive" style="--c:{cssColor(h.color) || 'var(--border-control)'}">
          <header>
            <span class="swatch"></span>
            <h3>{h.name}</h3>
            <span class="count" class:over={h.item_ids.length > cap}>{h.item_ids.length}/{cap}</span>
          </header>
          <div class="chips">
            {#each h.item_ids.slice(0, 18) as id (id)}
              {@const it = itemById.get(id)}
              {#if it}<span class="tk">{it.ticker}</span>{/if}
            {/each}
            {#if h.item_ids.length > 18}<span class="more">+{h.item_ids.length - 18}</span>{/if}
            {#if !h.item_ids.length}<span class="more">{$t('scanner.hives.noItems')}</span>{/if}
          </div>
          <footer>
            <Button size="sm" icon="list" onclick={() => openMembers(h)}>{$t('scanner.hives.members')}</Button>
            <Button size="sm" icon="pencil" onclick={() => (editing = { id: h.id, name: h.name, color: h.color })}>{$t('scanner.edit')}</Button>
            <button class="icon" title={$t('scanner.delete')} aria-label={$t('scanner.delete')} onclick={() => (deleting = h)}><Icon name="trash" size={13} /></button>
          </footer>
        </section>
      {/each}
    </div>
  {/if}
</div>

<Modal open={!!editing} title={editing?.id ? $t('scanner.hives.edit') : $t('scanner.hives.new')} onclose={() => (editing = null)}>
  {#if editing}
    <div class="form">
      <Input label={$t('scanner.name')} bind:value={editing.name} required />
      <div class="field">
        <span>{$t('scanner.color')}</span>
        <ColorSwatches bind:value={editing.color} />
      </div>
    </div>
  {/if}
  {#snippet footer()}
    <Button onclick={() => (editing = null)}>{$t('scanner.cancel')}</Button>
    <Button variant="primary" disabled={!editing?.name?.trim()} onclick={save}>{$t('scanner.save')}</Button>
  {/snippet}
</Modal>

<Modal open={!!members} size="md" title={members ? $t('scanner.hives.membersOf', { name: members.hive.name }) : ''} onclose={() => (members = null)}>
  {#if members}
    <div class="form">
      <input type="search" placeholder={$t('scanner.items.filter')} aria-label={$t('scanner.items.filter')} bind:value={members.q} />
      <p class="hint" class:bad={memberCount > cap}>{$t('scanner.hives.picked', { n: memberCount, cap })}</p>
      {#if !items.length}
        <p class="hint">{$t('scanner.hives.noUniverse')}</p>
      {/if}
      <ul class="pick">
        {#each memberRows as it (it.id)}
          <li>
            <label>
              <input type="checkbox" bind:checked={members.picked[it.id]} />
              <span class="tk">{it.ticker}</span>
              <span class="more">{it.timeframe} · {it.provider}</span>
            </label>
            {#if members.picked[it.id]}
              <button class="icon" title={$t('scanner.hives.removeMember')} aria-label={$t('scanner.hives.removeMember')} onclick={() => (members.picked[it.id] = false)}><Icon name="x" size={13} /></button>
            {/if}
          </li>
        {/each}
      </ul>
    </div>
  {/if}
  {#snippet footer()}
    <Button onclick={() => (members = null)}>{$t('scanner.cancel')}</Button>
    <Button variant="primary" onclick={saveMembers}>{$t('scanner.save')}</Button>
  {/snippet}
</Modal>

<Modal bind:open={capOpen} title={$t('scanner.hives.changeCap')}>
  <div class="form">
    <Input type="number" label={$t('scanner.hives.capLabel')} hint={$t('scanner.hives.capHint')} bind:value={capDraft} />
  </div>
  {#snippet footer()}
    <Button onclick={() => (capOpen = false)}>{$t('scanner.cancel')}</Button>
    <Button variant="primary" onclick={saveCap}>{$t('scanner.save')}</Button>
  {/snippet}
</Modal>

<ConfirmModal
  open={!!deleting}
  title={$t('scanner.hives.deleteTitle')}
  message={$t('scanner.hives.deleteBody', { name: deleting?.name ?? '' })}
  confirmLabel={$t('scanner.delete')}
  cancelLabel={$t('scanner.cancel')}
  danger
  onconfirm={remove}
  oncancel={() => (deleting = null)}
/>

<style>
  .wrap {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .cap {
    color: var(--dim);
    font-size: var(--text-xs);
    margin-left: auto;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
    gap: var(--space-3);
  }
  .hive {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    border-left: 3px solid var(--c);
  }
  header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .swatch {
    width: 10px;
    height: 10px;
    border-radius: 2px;
    background: var(--c);
  }
  h3 {
    flex: 1;
    margin: 0;
  }
  .count {
    font-family: var(--mono);
    font-size: var(--text-xs);
    color: var(--dim);
  }
  .count.over,
  .bad {
    color: var(--red);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
    min-height: 22px;
  }
  .tk {
    font-family: var(--mono);
    font-size: var(--text-xs);
  }
  .chips .tk {
    background: var(--surface-2);
    border-radius: var(--radius-sm);
    padding: 1px var(--space-2);
  }
  .more {
    color: var(--dim);
    font-size: var(--text-xs);
  }
  footer {
    display: flex;
    gap: var(--space-2);
    align-items: center;
  }
  .icon {
    margin-left: auto;
    background: none;
    border: none;
    color: var(--dim);
    cursor: pointer;
  }
  .icon:hover {
    color: var(--red);
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .pick {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 360px;
    overflow-y: auto;
  }
  .pick li {
    display: flex;
    align-items: center;
  }
  .pick label {
    flex: 1;
    display: flex;
    gap: var(--space-2);
    align-items: center;
    padding: var(--space-1) 0;
  }
</style>
