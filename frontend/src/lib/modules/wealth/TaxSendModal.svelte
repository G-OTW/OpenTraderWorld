<script>
  // Send to Tax Calculator: pick the categories that make up the wealth-tax base (a primary
  // residence is often exempt, a debt often deductible), then open the calculator with the
  // sum prefilled. The amount is in the display currency; the profile must use the same.
  import Modal from '$lib/ui/Modal.svelte';
  import { goto } from '$app/navigation';
  import { money } from './view.js';
  import { taxLink } from './engine.js';
  import { t } from '$lib/i18n';

  let { open = $bindable(false), byCategory = {}, currency = 'USD' } = $props();

  let picked = $state(new Set());
  $effect(() => {
    if (open) picked = new Set(Object.keys(byCategory));
  });

  function toggle(c) {
    const s = new Set(picked);
    if (s.has(c)) s.delete(c);
    else s.add(c);
    picked = s;
  }

  const link = $derived(taxLink(byCategory, picked, currency));
  const label = (c) => c || $t('wealth.page.uncategorized');
</script>

<Modal bind:open size="sm" title={$t('wealth.tax.title')}>
  <p class="muted">{$t('wealth.tax.intro', { currency })}</p>
  <ul class="list">
    {#each Object.entries(byCategory) as [c, v] (c)}
      <li>
        <label>
          <input type="checkbox" checked={picked.has(c)} onchange={() => toggle(c)} />
          <span class="name">{label(c)}</span>
          <span class="val" class:off={!picked.has(c)}>{money(v, currency)}</span>
        </label>
      </li>
    {/each}
  </ul>
  <div class="base">
    <span class="lbl">{$t('wealth.tax.base')}</span>
    <strong>{money(link.base, currency)}</strong>
  </div>
  {#snippet footer()}
    <button class="btn ghost" onclick={() => (open = false)}>{$t('common.cancel')}</button>
    <button class="btn primary" onclick={() => goto(link.href)} disabled={picked.size === 0}>{$t('wealth.tax.open')}</button>
  {/snippet}
</Modal>

<style>
  .muted {
    margin: 0;
    color: var(--muted);
    font-size: 12px;
    line-height: var(--lh-base);
  }
  .list {
    list-style: none;
    margin: var(--space-3) 0;
    padding: 0;
  }
  .list li {
    border-top: var(--hairline) solid var(--border);
  }
  .list label {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-height: 36px;
    cursor: pointer;
    font-size: 13px;
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .val,
  .base strong {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    text-align: right;
  }
  .val.off {
    color: var(--dim);
  }
  .base {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-2);
    padding-top: var(--space-3);
    border-top: var(--hairline) solid var(--border-control);
  }
  .base strong {
    font-size: 15px;
    font-weight: 600;
  }
  .lbl {
    color: var(--dim);
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  @media (max-width: 767px) {
    .list label {
      min-height: 44px;
    }
  }
</style>
