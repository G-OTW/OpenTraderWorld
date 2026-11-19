<script>
  // Row fields of one symbol: flag, section, buy and sell targets, and the virtual position
  // ("if I had bought N at P on D"). Empty fields clear; nothing here touches a portfolio.
  import Modal from '$lib/ui/Modal.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Field from '$lib/ui/Field.svelte';
  import Button from '$lib/ui/Button.svelte';
  import { watchlistsApi } from './api.js';
  import { FLAGS } from './views.js';
  import { t } from '$lib/i18n';

  let { open = $bindable(false), item = null, sections = [], onsaved = () => {} } = $props();

  let f = $state({});
  let busy = $state(false);
  let error = $state('');

  const str = (v) => (v == null ? '' : String(v));
  $effect(() => {
    if (!open || !item) return;
    f = {
      flag: item.flag ?? null,
      section: item.section ?? '',
      target_buy: str(item.target_buy),
      target_sell: str(item.target_sell),
      virt_qty: str(item.virt_qty),
      virt_price: str(item.virt_price),
      virt_date: item.virt_date ?? ''
    };
    error = '';
  });

  const numOrNull = (s) => (String(s).trim() === '' ? null : Number(s));
  const today = () => new Date().toISOString().slice(0, 10);

  async function save() {
    busy = true;
    error = '';
    try {
      const updated = await watchlistsApi.updateItem(item.id, {
        flag: f.flag,
        section: f.section.trim() || null,
        target_buy: numOrNull(f.target_buy),
        target_sell: numOrNull(f.target_sell),
        virt_qty: numOrNull(f.virt_qty),
        virt_price: numOrNull(f.virt_price),
        virt_date: f.virt_date || null
      });
      open = false;
      onsaved(updated);
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }
</script>

<Modal bind:open size="sm" title={$t('watchlists.edit.title', { symbol: item?.symbol ?? '' })}>
  <div class="form">
    <div class="flagrow" role="group" aria-label={$t('watchlists.edit.flag')}>
      <span class="lbl">{$t('watchlists.edit.flag')}</span>
      <div class="flags">
        <button class="flag" class:on={!f.flag} onclick={() => (f.flag = null)} title={$t('watchlists.edit.noFlag')} aria-label={$t('watchlists.edit.noFlag')} aria-pressed={!f.flag}>
          <span class="dot none"></span>
        </button>
        {#each FLAGS as fl (fl.id)}
          <button class="flag" class:on={f.flag === fl.id} onclick={() => (f.flag = fl.id)} title={$t(`watchlists.flag.${fl.id}`)} aria-label={$t(`watchlists.flag.${fl.id}`)} aria-pressed={f.flag === fl.id}>
            <span class="dot" style:background={fl.color}></span>
          </button>
        {/each}
      </div>
    </div>

    <Field id="wl-edit-section" label={$t('watchlists.edit.section')}>
      <input id="wl-edit-section" list="wl-sections" bind:value={f.section} maxlength="60" placeholder={$t('watchlists.edit.sectionNone')} />
      <datalist id="wl-sections">
        {#each sections as s (s)}<option value={s}></option>{/each}
      </datalist>
    </Field>

    <div class="pair">
      <Field id="wl-edit-buy" label={$t('watchlists.edit.targetBuy')}>
        <input id="wl-edit-buy" class="num" type="number" step="any" min="0" bind:value={f.target_buy} />
      </Field>
      <Field id="wl-edit-sell" label={$t('watchlists.edit.targetSell')}>
        <input id="wl-edit-sell" class="num" type="number" step="any" min="0" bind:value={f.target_sell} />
      </Field>
    </div>

    <div class="group">
      <p class="hint">{$t('watchlists.edit.virtHint')}</p>
      <div class="pair three">
        <Field id="wl-edit-qty" label={$t('watchlists.edit.virtQty')}>
          <input id="wl-edit-qty" class="num" type="number" step="any" min="0" bind:value={f.virt_qty} />
        </Field>
        <Field id="wl-edit-price" label={$t('watchlists.edit.virtPrice')}>
          <input id="wl-edit-price" class="num" type="number" step="any" min="0" bind:value={f.virt_price} />
        </Field>
        <Field id="wl-edit-date" label={$t('watchlists.edit.virtDate')}>
          <input id="wl-edit-date" type="date" max={today()} bind:value={f.virt_date} />
        </Field>
      </div>
    </div>

    <ErrorText {error} copyable />
  </div>

  {#snippet footer()}
    <Button variant="ghost" onclick={() => (open = false)}>{$t('common.cancel')}</Button>
    <Button variant="primary" onclick={save} loading={busy}>{$t('common.save')}</Button>
  {/snippet}
</Modal>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .flagrow {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  /* Same label as the shared Field. */
  .lbl {
    font-size: 12px;
    font-weight: 500;
    color: var(--muted);
    line-height: var(--lh-tight);
  }
  .pair {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-3);
  }
  .pair.three {
    grid-template-columns: 1fr 1fr 1fr;
  }
  .pair :global(input) {
    width: 100%;
    min-width: 0;
  }
  .num {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  .group {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding-top: var(--space-3);
    border-top: var(--hairline) solid var(--border);
  }
  .flags {
    display: flex;
    gap: var(--space-1);
  }
  .flag {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: transparent;
    border: var(--hairline) solid transparent;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  .flag:hover {
    background: var(--surface-hover);
  }
  .flag.on {
    border-color: var(--accent);
    background: var(--accent-soft);
  }
  .dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
  }
  .dot.none {
    background: transparent;
    border: 1px solid var(--faint);
  }
  .hint {
    font-size: 12px;
    color: var(--dim);
    margin: 0;
    line-height: 1.4;
  }
  @media (max-width: 520px) {
    .pair,
    .pair.three {
      grid-template-columns: 1fr;
    }
  }
</style>
