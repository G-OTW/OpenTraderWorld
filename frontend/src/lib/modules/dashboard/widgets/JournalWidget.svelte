<script>
  // Quick-trade widget: pick a category + template, then open the full add-trade modal
  // (reusing the journal's TradeForm). On save the trade is created via the journal API.
  import Modal from '$lib/ui/Modal.svelte';
  import TradeForm from '$lib/modules/journal/TradeForm.svelte';
  import { journalApi } from '$lib/modules/journal/api.js';
  import { t } from '$lib/i18n';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import WidgetState from './WidgetState.svelte';

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

<WidgetState
  {editing}
  error={err}
  loading={categories === null}
  preview={$t('dashboard.widgets.journal.preview')}
  rows={3}
>
  <div class="form">
    <div class="fld">
      <span class="w-eyebrow">{$t('dashboard.widgets.journal.category')}</span>
      <Dropdown
        bind:value={categoryId}
        ariaLabel={$t('dashboard.widgets.journal.category')}
        options={categories.map((c) => ({ value: c.id, label: c.name }))}
      />
    </div>
    <div class="fld">
      <span class="w-eyebrow">{$t('dashboard.widgets.journal.template')}</span>
      <Dropdown
        bind:value={templateId}
        ariaLabel={$t('dashboard.widgets.journal.template')}
        options={[
          { value: '', label: $t('dashboard.widgets.journal.adHoc') },
          ...(templates ?? []).map((tpl) => ({ value: tpl.id, label: tpl.name }))
        ]}
      />
    </div>
    <button class="primary add" onclick={() => (formOpen = true)} disabled={!categoryId}>{$t('dashboard.widgets.journal.addTrade')}</button>
  </div>
</WidgetState>

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
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .fld {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }
  /* The one promoted action in the widget: everything above it is a picker. */
  .add {
    margin-top: var(--space-1);
    width: 100%;
  }
</style>
