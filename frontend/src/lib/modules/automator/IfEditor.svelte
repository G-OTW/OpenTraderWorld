<script>
  // `if` block: the only branch in a workflow. Both sides are expressions, so a condition
  // reads the same way a block field does.
  import Select from '$lib/ui/Select.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import ExprField from './ExprField.svelte';
  import { t } from '$lib/i18n';

  let { config = $bindable({}), catalog = null, sources = [] } = $props();

  const ops = $derived(
    (catalog?.ops?.condition ?? ['eq', 'ne']).map((o) => ({
      value: o,
      label: $t(`automator.op.${o}`)
    }))
  );
  // Operators that read one side only; the right field disappears for them.
  const UNARY = ['empty', 'not_empty', 'is_true'];

  function add() {
    config.conditions = [...(config.conditions ?? []), { left: '', op: 'eq', right: '' }];
  }
  function remove(i) {
    config.conditions = (config.conditions ?? []).filter((_, j) => j !== i);
  }
</script>

<div class="stack">
  <Select
    label={$t('automator.if.match')}
    options={[
      { value: 'all', label: $t('automator.if.all') },
      { value: 'any', label: $t('automator.if.any') }
    ]}
    bind:value={config.match}
  />

  {#each config.conditions ?? [] as cond, i (i)}
    <div class="cond">
      <ExprField bind:value={cond.left} placeholder={'{{steps.api1.output.body.count}}'} {sources} />
      <Select options={ops} bind:value={cond.op} />
      {#if !UNARY.includes(cond.op)}
        <ExprField bind:value={cond.right} placeholder="10" {sources} />
      {:else}
        <span class="spacer"></span>
      {/if}
      <button type="button" class="icon" onclick={() => remove(i)} title={$t('common.delete')}>
        <Icon name="x" size={12} />
      </button>
    </div>
  {/each}

  <button type="button" class="link" onclick={add}>
    <Icon name="plus" size={12} />
    {$t('automator.if.add')}
  </button>
</div>

<style>
  .stack {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .cond {
    display: grid;
    grid-template-columns: 1fr 130px 1fr 24px;
    gap: var(--space-2);
    align-items: center;
  }
  .spacer {
    display: block;
  }
  .icon {
    background: none;
    border: 0;
    color: var(--muted);
    cursor: pointer;
  }
  .icon:hover {
    color: var(--red);
  }
  .link {
    align-self: flex-start;
    display: inline-flex;
    align-items: center;
    gap: 3px;
    background: none;
    border: 0;
    color: var(--accent);
    font-size: var(--text-xs);
    cursor: pointer;
    padding: 0;
  }
</style>
