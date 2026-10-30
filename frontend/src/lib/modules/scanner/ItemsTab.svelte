<script>
  // The universe: every item, its hives, its color and what it last saw. Items are added with
  // the symbol lookup or imported from a chart list; membership is edited by selecting rows
  // and adding them to (or taking them out of) a hive, which the cap guards.
  import Button from '$lib/ui/Button.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Input from '$lib/ui/Input.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import AddItemModal from './AddItemModal.svelte';
  import ImportListModal from './ImportListModal.svelte';
  import ColorSwatches from './ColorSwatches.svelte';
  import { scannerApi, chartUrl } from './api.js';
  import { cssColor, itemColor } from './board.js';
  import { fmtNum } from '$lib/format.js';
  import { t } from '$lib/i18n';

  let { items = [], hives = [], cap = 200, onchanged } = $props();

  let filter = $state('');
  let selected = $state({});
  let hiveId = $state('');
  let error = $state('');
  let busy = $state(false);
  let addOpen = $state(false);
  let importOpen = $state(false);
  let editing = $state(null);
  let editLabel = $state('');
  let editColor = $state('');
  let confirmOpen = $state(false);

  const shown = $derived(
    items.filter((it) => {
      const f = filter.trim().toLowerCase();
      return !f || it.ticker.toLowerCase().includes(f) || (it.label ?? '').toLowerCase().includes(f);
    })
  );
  const picked = $derived(items.filter((it) => selected[it.id]));
  const allOn = $derived(shown.length > 0 && shown.every((it) => selected[it.id]));
  const hiveName = (id) => hives.find((h) => h.id === id)?.name ?? '';

  function toggleAll() {
    const on = !allOn;
    const next = { ...selected };
    for (const it of shown) next[it.id] = on;
    selected = next;
  }

  async function membership(add) {
    const h = hives.find((x) => x.id === hiveId);
    if (!h || !picked.length) return;
    const ids = new Set(h.item_ids);
    for (const it of picked) add ? ids.add(it.id) : ids.delete(it.id);
    busy = true;
    try {
      await scannerApi.setHiveItems(h.id, [...ids]);
      error = '';
      onchanged?.();
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }

  function edit(it) {
    editing = it;
    editLabel = it.label ?? '';
    editColor = it.color ?? '';
  }

  async function saveEdit() {
    const it = editing;
    try {
      await scannerApi.saveItem(it.id, {
        provider: it.provider,
        asset_type: it.asset_type,
        ticker: it.ticker,
        timeframe: it.timeframe,
        connector_id: it.connector_id,
        label: editLabel,
        color: editColor
      });
      editing = null;
      onchanged?.();
    } catch (e) {
      error = e.message;
    }
  }

  async function removePicked() {
    busy = true;
    try {
      for (const it of picked) await scannerApi.deleteItem(it.id);
      selected = {};
      error = '';
      onchanged?.();
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }
</script>

<div class="wrap">
  <div class="filter-bar">
    <Button variant="primary" icon="plus" onclick={() => (addOpen = true)}>{$t('scanner.items.add')}</Button>
    <Button icon="download" onclick={() => (importOpen = true)}>{$t('scanner.items.import')}</Button>
    <input class="find" type="search" placeholder={$t('scanner.items.filter')} aria-label={$t('scanner.items.filter')} bind:value={filter} />
    {#if picked.length}
      <span class="sel">{$t('scanner.items.selected', { n: picked.length })}</span>
      <div class="dd">
        <Dropdown
          bind:value={hiveId}
          ariaLabel={$t('scanner.items.hive')}
          placeholder={$t('scanner.items.pickHive')}
          options={hives.map((h) => ({ value: h.id, label: `${h.name} (${h.item_ids.length}/${cap})` }))}
        />
      </div>
      <Button size="sm" disabled={!hiveId || busy} onclick={() => membership(true)}>{$t('scanner.items.toHive')}</Button>
      <Button size="sm" disabled={!hiveId || busy} onclick={() => membership(false)}>{$t('scanner.items.fromHive')}</Button>
      <Button size="sm" variant="danger" icon="trash" disabled={busy} onclick={() => (confirmOpen = true)}>{$t('scanner.delete')}</Button>
    {/if}
  </div>

  {#if error}<ErrorText {error} />{/if}

  {#if !items.length}
    <EmptyState icon="radio" title={$t('scanner.items.emptyTitle')} description={$t('scanner.items.emptyBody')}>
      {#snippet action()}
        <Button variant="primary" icon="plus" onclick={() => (addOpen = true)}>{$t('scanner.items.add')}</Button>
      {/snippet}
    </EmptyState>
  {:else}
    <div class="scroll">
      <table class="tbl">
        <thead>
          <tr>
            <th><input type="checkbox" checked={allOn} onchange={toggleAll} aria-label={$t('scanner.items.selectAll')} /></th>
            <th>{$t('scanner.items.instrument')}</th>
            <th>{$t('scanner.timeframe')}</th>
            <th>{$t('scanner.items.hives')}</th>
            <th class="num">{$t('scanner.col.close')}</th>
            <th>{$t('scanner.items.status')}</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each shown as it (it.id)}
            <tr>
              <td><input type="checkbox" bind:checked={selected[it.id]} aria-label={it.ticker} /></td>
              <td>
                <span class="dot" style="--c:{cssColor(itemColor(it, hives)) || 'var(--border-control)'}"></span>
                <a class="sym" href={chartUrl(it)}>{it.ticker}</a>
                {#if it.label}<span class="lbl">{it.label}</span>{/if}
                <span class="lbl">{it.provider}</span>
              </td>
              <td>{it.timeframe}</td>
              <td class="hv">{it.hive_ids.map(hiveName).filter(Boolean).join(', ')}</td>
              <td class="num">{it.last_close != null ? fmtNum(it.last_close, 4) : ''}</td>
              <td>
                {#if it.last_error}
                  <span class="err" title={it.last_error}><Icon name="alert-triangle" size={12} /> {it.last_error}</span>
                {:else if it.last_bar_ts}
                  <span class="lbl">{$t('scanner.items.seen', { at: new Date(it.last_bar_ts).toLocaleString() })}</span>
                {:else}
                  <span class="lbl">{$t('scanner.items.notRead')}</span>
                {/if}
              </td>
              <td class="act">
                <button class="icon" title={$t('scanner.edit')} aria-label={$t('scanner.edit')} onclick={() => edit(it)}><Icon name="pencil" size={13} /></button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</div>

<AddItemModal bind:open={addOpen} {hives} {onchanged} />
<ImportListModal bind:open={importOpen} {hives} {onchanged} />

<Modal open={!!editing} title={editing ? `${editing.ticker} ${editing.timeframe}` : ''} onclose={() => (editing = null)}>
  {#if editing}
    <div class="form">
      <Input label={$t('scanner.items.label')} bind:value={editLabel} />
      <div class="field">
        <span>{$t('scanner.color')}</span>
        <ColorSwatches bind:value={editColor} />
        <span class="hint">{$t('scanner.items.colorHint')}</span>
      </div>
    </div>
  {/if}
  {#snippet footer()}
    <Button onclick={() => (editing = null)}>{$t('scanner.cancel')}</Button>
    <Button variant="primary" onclick={saveEdit}>{$t('scanner.save')}</Button>
  {/snippet}
</Modal>

<ConfirmModal
  bind:open={confirmOpen}
  title={$t('scanner.items.deleteTitle')}
  message={$t('scanner.items.deleteBody', { n: picked.length })}
  confirmLabel={$t('scanner.delete')}
  cancelLabel={$t('scanner.cancel')}
  danger
  onconfirm={removePicked}
/>

<style>
  .wrap {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-width: 0;
  }
  .find {
    flex: 1 1 160px;
    max-width: 260px;
  }
  .sel {
    color: var(--dim);
    font-size: var(--text-xs);
  }
  .scroll {
    overflow-x: auto;
  }
  .dot {
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--c);
    margin-right: var(--space-1);
  }
  .sym {
    font-family: var(--mono);
    color: var(--text);
    text-decoration: none;
  }
  .sym:hover {
    text-decoration: underline;
  }
  .lbl {
    color: var(--dim);
    font-size: var(--text-xs);
    margin-left: var(--space-2);
  }
  .hv {
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .err {
    color: var(--red);
    font-size: var(--text-xs);
    display: inline-flex;
    gap: var(--space-1);
    align-items: center;
    max-width: 320px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .act {
    text-align: right;
  }
  .icon {
    background: none;
    border: none;
    color: var(--dim);
    cursor: pointer;
    padding: var(--space-1);
  }
  .icon:hover {
    color: var(--text);
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
</style>
