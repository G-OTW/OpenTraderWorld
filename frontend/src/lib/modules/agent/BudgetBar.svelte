<script>
  // This chat's spend against its limits: a thin bar filling bottom to top with the larger of
  // the two ratios (output tokens, $). Empty when the chat is unlimited. Hover shows the
  // figures; click opens a small editor for this chat's limits.
  import { t } from '$lib/i18n';
  import Button from '$lib/ui/Button.svelte';
  import Input from '$lib/ui/Input.svelte';
  import { clickOutside } from '$lib/ui/clickOutside.js';

  // props: conv ({ limit_output_tokens, limit_cost_usd }), usage ({ output_tokens, cost_usd }),
  //        onsave(limits) → Promise (limits = { output_tokens, cost_usd }, null = unlimited),
  //        open (bindable: the parent opens the editor from the budget-reached banner)
  let { conv, usage, onsave, open = $bindable(false) } = $props();

  const tokLimit = $derived(conv?.limit_output_tokens ?? null);
  const costLimit = $derived(conv?.limit_cost_usd ?? null);
  const outTokens = $derived(usage?.output_tokens || 0);
  // null = the provider never reported a price for this chat.
  const cost = $derived(usage?.cost_usd ?? null);

  const ratio = $derived.by(() => {
    const r = [];
    if (tokLimit) r.push(outTokens / tokLimit);
    if (costLimit) r.push((cost ?? 0) / costLimit);
    return r.length ? Math.min(1, Math.max(...r)) : 0;
  });
  const tone = $derived(ratio >= 1 ? 'over' : ratio >= 0.8 ? 'warn' : '');

  const fmtUsd = (v) => `$${v < 1 ? v.toFixed(4) : v.toFixed(2)}`;

  let tokIn = $state('');
  let costIn = $state('');
  let saving = $state(false);
  let err = $state('');

  // Fill the editor with the current limits each time it opens, whoever opened it.
  let wasOpen = false;
  $effect(() => {
    if (open && !wasOpen) {
      tokIn = tokLimit ?? '';
      costIn = costLimit ?? '';
      err = '';
    }
    wasOpen = open;
  });

  function toggle() {
    open = !open;
  }

  const num = (v) => (v === '' || v == null ? null : Number(v));

  async function save(limits) {
    saving = true;
    err = '';
    try {
      await onsave(limits);
      open = false;
    } catch (e) {
      err = e.message;
    } finally {
      saving = false;
    }
  }
</script>

<span class="budget" use:clickOutside={() => (open = false)}>
  <button
    type="button"
    class="bar {tone}"
    class:on={open}
    aria-label={$t('agent.budget.aria')}
    aria-expanded={open}
    onclick={toggle}
  >
    <span class="fill" style:height="{ratio * 100}%"></span>
  </button>

  {#if !open}
    <div class="hover" role="tooltip">
      <p class="title">{$t('agent.budget.title')}</p>
      <p>
        {$t('agent.budget.tokens')}:
        <b>{outTokens.toLocaleString()}</b>
        {#if tokLimit}/ {tokLimit.toLocaleString()}{:else}<span class="muted">({$t('agent.budget.unlimited')})</span>{/if}
      </p>
      <p>
        {$t('agent.budget.cost')}:
        {#if cost != null}<b>{fmtUsd(cost)}</b>{:else}<span class="muted">{$t('agent.budget.noPrice')}</span>{/if}
        {#if costLimit}/ {fmtUsd(costLimit)}{:else}<span class="muted">({$t('agent.budget.unlimited')})</span>{/if}
      </p>
      <p class="muted">{$t('agent.budget.clickHint')}</p>
    </div>
  {:else}
    <div class="pop" role="dialog" aria-label={$t('agent.budget.title')}>
      <p class="title">{$t('agent.budget.title')}</p>
      <Input
        label={$t('agent.set.limitTokens')}
        type="number"
        min="1"
        step="1000"
        placeholder={$t('agent.set.unlimited')}
        bind:value={tokIn}
      />
      <Input
        label={$t('agent.set.limitCost')}
        type="number"
        min="0.01"
        step="0.01"
        placeholder={$t('agent.set.unlimited')}
        bind:value={costIn}
      />
      {#if err}<p class="err">{err}</p>{/if}
      <div class="row">
        <Button size="sm" variant="primary" loading={saving} onclick={() => save({ output_tokens: num(tokIn), cost_usd: num(costIn) })}>
          {$t('common.save')}
        </Button>
        <Button size="sm" variant="ghost" disabled={saving} onclick={() => save({ output_tokens: null, cost_usd: null })}>
          {$t('agent.budget.removeLimits')}
        </Button>
      </div>
    </div>
  {/if}
</span>

<style>
  .budget {
    position: relative;
    display: inline-flex;
    align-items: center;
  }
  .bar {
    position: relative;
    width: 8px;
    height: 28px;
    padding: 0;
    margin: 0 var(--space-1);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: 3px;
    overflow: hidden;
    cursor: pointer;
  }
  .bar:hover,
  .bar.on {
    border-color: var(--muted);
  }
  .fill {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    background: var(--accent);
    transition: height 0.3s ease;
  }
  .bar.warn .fill {
    background: var(--amber);
  }
  .bar.over .fill {
    background: var(--red);
  }
  .hover,
  .pop {
    position: absolute;
    bottom: calc(100% + var(--space-2));
    left: 0;
    z-index: var(--z-dropdown);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    box-shadow: 0 8px 24px rgb(0 0 0 / 0.18);
    font-size: var(--text-sm);
    color: var(--text);
    padding: var(--space-3);
  }
  .hover {
    display: none;
    width: max-content;
    max-width: min(280px, calc(100vw - var(--space-4)));
    pointer-events: none;
  }
  .budget:hover .hover {
    display: block;
  }
  .pop {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    width: 240px;
    max-width: calc(100vw - var(--space-4));
  }
  p {
    margin: 0 0 var(--space-1);
  }
  .pop p {
    margin: 0;
  }
  .hover p:last-child {
    margin-bottom: 0;
  }
  .title {
    font-weight: var(--fw-medium);
  }
  .muted {
    color: var(--muted);
  }
  .err {
    color: var(--red);
    font-size: var(--text-xs);
  }
  .row {
    display: flex;
    gap: var(--space-2);
  }
</style>
