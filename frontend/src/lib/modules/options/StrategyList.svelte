<script>
  // Saved strategies: save the legs being built (new or over the loaded one), load one back,
  // duplicate, delete after a confirm, and add one to the comparison overlay.
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import PromptModal from '$lib/ui/PromptModal.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { optionsApi } from './api.js';
  import { opt, legMissing } from './store.svelte.js';
  import { t } from '$lib/i18n';

  let { onload = () => {}, oncompare = () => {}, comparing = [] } = $props();

  let list = $state([]);
  let error = $state('');
  let saveOpen = $state(false);
  let saveAs = $state(false);
  let toDelete = $state(null);
  let confirmOpen = $state(false);

  async function reload() {
    try {
      list = await optionsApi.strategies.list();
      error = '';
    } catch (e) {
      error = e.message;
    }
  }
  $effect(() => {
    reload();
  });

  const unwrap = (r) => r?.strategy ?? r;
  const bareLegs = (legs) => legs.map(({ kind, strike, expiry, qty, price, iv }) => ({ kind, strike, expiry, qty, price, iv }));

  // The first leg that cannot be saved yet, and the field it lacks.
  const FIELD_LABEL = { price: 'options.legs.price', strike: 'options.col.strike', expiry: 'options.legs.expiry' };
  const incomplete = $derived.by(() => {
    const i = opt.legs.findIndex((l) => legMissing(l));
    return i < 0 ? null : { n: i + 1, field: FIELD_LABEL[legMissing(opt.legs[i])] };
  });
  const canSave = $derived(opt.legs.length > 0 && !incomplete);

  const current = $derived(list.find((s) => s.id === opt.strategyId) ?? null);
  const saveFields = $derived([
    { key: 'name', label: $t('options.saved.name'), value: saveAs ? '' : (current?.name ?? opt.strategyName), required: true },
    { key: 'notes', label: $t('options.saved.notes'), value: saveAs ? '' : (current?.notes ?? '') }
  ]);

  function askSave(asNew) {
    saveAs = asNew || !opt.strategyId;
    saveOpen = true;
  }

  async function save({ name, notes }) {
    const body = {
      name: name.trim(),
      underlying: opt.underlying?.symbol ?? current?.underlying ?? '',
      asset_type: opt.underlying?.asset_type ?? opt.assetType,
      provider: opt.provider,
      legs: bareLegs(opt.legs),
      notes: notes ?? ''
    };
    try {
      const r = unwrap(saveAs ? await optionsApi.strategies.create(body) : await optionsApi.strategies.update(opt.strategyId, body));
      if (r?.id) opt.strategyId = r.id;
      opt.strategyName = body.name;
      await reload();
    } catch (e) {
      error = e.message;
    }
  }

  async function duplicate(s) {
    try {
      await optionsApi.strategies.create({
        name: $t('options.saved.copyOf', { name: s.name }),
        underlying: s.underlying,
        asset_type: s.asset_type,
        provider: s.provider,
        legs: s.legs,
        notes: s.notes ?? ''
      });
      await reload();
    } catch (e) {
      error = e.message;
    }
  }

  async function remove() {
    const s = toDelete;
    toDelete = null;
    if (!s) return;
    try {
      await optionsApi.strategies.remove(s.id);
      if (opt.strategyId === s.id) {
        opt.strategyId = null;
        opt.strategyName = '';
      }
      await reload();
    } catch (e) {
      error = e.message;
    }
  }
</script>

<section class="qp-block">
  <div class="head">
    <h3>{$t('options.saved.title')}</h3>
    <div class="actions">
      {#if opt.strategyId}
        <button class="ghost" onclick={() => askSave(true)} disabled={!canSave}>{$t('options.saved.saveAs')}</button>
      {/if}
      <button class="primary" onclick={() => askSave(false)} disabled={!canSave}><Icon name="save" size={14} /> {$t('options.saved.save')}</button>
    </div>
  </div>
  {#if incomplete}<p class="qp-hint">{$t('options.saved.incomplete', { n: incomplete.n, field: $t(incomplete.field) })}</p>{/if}
  <ErrorText {error} />
  {#if list.length}
    <ul class="list">
      {#each list as s (s.id)}
        <li class:active={s.id === opt.strategyId}>
          <button class="name" onclick={() => onload(s)} title={s.notes || ''}>
            <strong>{s.name}</strong>
            <span class="qp-muted">{s.underlying} · {$t('options.saved.legs', { n: s.legs?.length ?? 0 })}</span>
          </button>
          <button class="icon" class:on={comparing.includes(s.id)} onclick={() => oncompare(s)} aria-label={$t('options.compare.toggle')} title={$t('options.compare.toggle')}><Icon name="git-compare" size={14} /></button>
          <button class="icon" onclick={() => duplicate(s)} aria-label={$t('options.saved.duplicate')} title={$t('options.saved.duplicate')}><Icon name="copy" size={14} /></button>
          <button class="icon danger-hover" onclick={() => ((toDelete = s), (confirmOpen = true))} aria-label={$t('options.saved.delete')} title={$t('options.saved.delete')}><Icon name="trash" size={14} /></button>
        </li>
      {/each}
    </ul>
  {:else}
    <p class="qp-hint">{$t('options.saved.empty')}</p>
  {/if}
</section>

<PromptModal bind:open={saveOpen} title={saveAs ? $t('options.saved.saveNew') : $t('options.saved.save')} fields={saveFields} confirmLabel={$t('options.saved.save')} onconfirm={save} />
<ConfirmModal
  bind:open={confirmOpen}
  title={$t('options.saved.deleteTitle')}
  message={$t('options.saved.deleteMsg', { name: toDelete?.name ?? '' })}
  confirmLabel={$t('options.saved.delete')}
  cancelLabel={$t('options.cancel')}
  danger
  onconfirm={remove}
  oncancel={() => (toDelete = null)}
/>

<style>
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }
  .actions {
    display: flex;
    gap: var(--space-2);
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
  }
  li {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    border-bottom: var(--hairline) solid var(--border);
  }
  li.active .name strong {
    color: var(--accent);
  }
  .name {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-1);
    background: none;
    border: none;
    color: var(--text);
    text-align: left;
    padding: var(--space-2) 0;
    cursor: pointer;
  }
  .icon.on {
    color: var(--accent);
  }
  @media (max-width: 767px) {
    .actions {
      flex-wrap: wrap;
    }
    .actions button {
      min-height: 44px;
      height: auto;
    }
    .name {
      min-height: 44px;
      overflow-wrap: anywhere;
    }
    li button.icon {
      flex: none;
      width: 44px;
      height: 44px;
    }
  }
</style>
