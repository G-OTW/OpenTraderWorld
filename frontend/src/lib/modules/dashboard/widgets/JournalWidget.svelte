<script>
  // Quick-trade widget: pick a category + template, then open the full add-trade modal
  // (reusing the journal's TradeForm). On save the trade is created via the journal API.
  import Modal from '$lib/ui/Modal.svelte';
  import TradeForm from '$lib/modules/journal/TradeForm.svelte';
  import { journalApi } from '$lib/modules/journal/api.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';

  let { item, editing } = $props();

  let categories = $state(null);
  let templates = $state(null);
  let strategies = $state([]);
  let feeSchedules = $state([]);
  let err = $state('');

  let categoryId = $state('');
  let templateId = $state('');
  let formOpen = $state(false);

  async function load() {
    err = '';
    try {
      [categories, templates, strategies, feeSchedules] = await Promise.all([
        journalApi.listCategories(),
        journalApi.listTemplates(),
        journalApi.listStrategies(),
        journalApi.listFeeSchedules()
      ]);
      categoryId ||= categories[0]?.id ?? '';
    } catch (e) {
      err = e.message;
    }
  }
  $effect(() => {
    if (!editing) load();
  });

  const template = $derived((templates ?? []).find((t) => t.id === templateId) ?? null);

  async function submit(trade) {
    try {
      await journalApi.addTrade(trade);
      formOpen = false;
    } catch (e) {
      err = e.message;
    }
  }
</script>

{#if editing}
  <p class="hint">{$t('dashboard.widgets.journal.preview')}</p>
{:else if err}
  <ErrorText error={err} compact />
{:else if categories === null}
  <div class="sk" aria-busy="true"><Skeleton rows={3} height="1.1rem" gap="var(--space-2)" /></div>
{:else}
  <div class="form">
    <label>
      <span>{$t('dashboard.widgets.journal.category')}</span>
      <select bind:value={categoryId}>
        {#each categories as c}<option value={c.id}>{c.name}</option>{/each}
      </select>
    </label>
    <label>
      <span>{$t('dashboard.widgets.journal.template')}</span>
      <select bind:value={templateId}>
        <option value="">{$t('dashboard.widgets.journal.adHoc')}</option>
        {#each templates ?? [] as tpl}<option value={tpl.id}>{tpl.name}</option>{/each}
      </select>
    </label>
    <button class="add" onclick={() => (formOpen = true)} disabled={!categoryId}>{$t('dashboard.widgets.journal.addTrade')}</button>
  </div>
{/if}

<Modal bind:open={formOpen} size="lg" title={$t('dashboard.widgets.journal.addTradeTitle')}>
  {#if formOpen}
    <TradeForm
      {template}
      categories={categories ?? []}
      {strategies}
      {feeSchedules}
      defaultCategoryId={categoryId}
      onsubmit={submit}
      oncancel={() => (formOpen = false)}
    />
  {/if}
</Modal>

<style>
  .sk {
    padding: var(--space-1) 0;
  }
  /* Preview, loading and empty text — not an error. This was grouped with a
     now-removed .err rule and inherited its red. */
  .hint {
    color: var(--dim);
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  label {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    font-size: var(--text-sm);
    color: var(--dim);
  }
  .add {
    margin-top: var(--space-2);
  }
</style>
