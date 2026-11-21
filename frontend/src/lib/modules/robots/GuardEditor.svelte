<script>
  // The robot's guards: rules over its own closed trades that pause it, halt it or scale its
  // size down for a while. One row per rule, read as a sentence: when this happens, do that.
  // The server checks every rule on save and names the fix when one does not hold.
  import Select from '$lib/ui/Select.svelte';
  import Input from '$lib/ui/Input.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';
  import { WHEN_KINDS, THEN_KINDS, RESTORE_KINDS, WINDOWS, whenField, windowed, newRule, setWhenKind, setThenKind } from './fleet.js';

  let { guards = $bindable([]), defaults = [] } = $props();

  const whenOptions = $derived(WHEN_KINDS.map((k) => ({ value: k, label: $t(`robots.guard.when.${k}`) })));
  const thenOptions = $derived(THEN_KINDS.map((k) => ({ value: k, label: $t(`robots.guard.then.${k}`) })));
  const restoreOptions = $derived(RESTORE_KINDS.map((k) => ({ value: k, label: $t(`robots.guard.restore.${k}`) })));
  const windowOptions = $derived(WINDOWS.map((k) => ({ value: k, label: $t(`robots.guard.window.${k}`) })));
  const fromOptions = $derived(['peak', 'start'].map((k) => ({ value: k, label: $t(`robots.guard.from.${k}`) })));

  function add() {
    guards = [...(guards ?? []), newRule()];
  }
  function remove(i) {
    guards = guards.filter((_, j) => j !== i);
  }
  function whenKind(i, kind) {
    guards[i].when = setWhenKind(guards[i].when, kind);
  }
  function thenKind(i, kind) {
    guards[i].then = setThenKind(kind);
  }
  function restoreKind(i, kind) {
    guards[i].then.restore = kind === 'manual' ? { kind } : { kind, n: guards[i].then.restore?.n ?? 5 };
  }
</script>

<div class="guards">
  {#each guards ?? [] as rule, i (i)}
    <div class="rule">
      <div class="head">
        <span class="no">{i + 1}</span>
        <button class="rm" type="button" title={$t('common.remove')} aria-label={$t('common.remove')} onclick={() => remove(i)}><Icon name="x" size={13} /></button>
      </div>
      <div class="row">
        <span class="lead">{$t('robots.guard.whenLead')}</span>
        <Select options={whenOptions} value={rule.when.kind} onpick={(v) => whenKind(i, v)} />
        <Input type="number" bind:value={rule.when[whenField(rule.when.kind)]} aria-label={$t(`robots.guard.field.${whenField(rule.when.kind)}`)} />
        {#if windowed(rule.when.kind)}
          <Select options={windowOptions} bind:value={rule.when.window} />
        {/if}
        {#if rule.when.kind === 'drawdown_pct'}
          <Select options={fromOptions} bind:value={rule.when.from} />
        {/if}
      </div>
      <div class="row">
        <span class="lead">{$t('robots.guard.thenLead')}</span>
        <Select options={thenOptions} value={rule.then.kind} onpick={(v) => thenKind(i, v)} />
        {#if rule.then.kind === 'pause'}
          <Input type="number" label={$t('robots.guard.days')} bind:value={rule.then.days} />
          <Input type="number" label={$t('robots.guard.hours')} bind:value={rule.then.hours} />
        {:else if rule.then.kind === 'scale'}
          <Input type="number" label={$t('robots.guard.divisor')} bind:value={rule.then.divisor} />
          <Select label={$t('robots.guard.restoreLabel')} options={restoreOptions} value={rule.then.restore?.kind ?? 'manual'} onpick={(v) => restoreKind(i, v)} />
          {#if rule.then.restore && rule.then.restore.kind !== 'manual'}
            <Input type="number" label={$t('robots.guard.count')} bind:value={rule.then.restore.n} />
          {/if}
        {/if}
      </div>
    </div>
  {:else}
    <p class="none">{$t('robots.guard.none')}</p>
  {/each}
  <div class="acts">
    <Button size="sm" icon="plus" onclick={add}>{$t('robots.guard.add')}</Button>
    {#if defaults?.length}
      <Button size="sm" variant="ghost" onclick={() => (guards = structuredClone(defaults))}>{$t('robots.guard.useDefaults')}</Button>
    {/if}
  </div>
  <p class="note">{$t('robots.guard.precedence')}</p>
</div>

<style>
  .guards {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  /* A rule sits inside a framed body (wizard, editor modal): hairline rules between rules
     instead of a box per rule, so no border lands inside a border. */
  .rule {
    border-top: var(--hairline) solid var(--border);
    padding-top: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .head {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  .no {
    font-family: var(--mono);
    font-size: var(--fs-metric-label);
    color: var(--dim);
  }
  .rm {
    border: 0;
    background: transparent;
    color: var(--dim);
    cursor: pointer;
    display: inline-flex;
    padding: var(--space-1);
  }
  .rm:hover {
    color: var(--red);
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: var(--space-2);
  }
  .row > :global(*) {
    min-width: 0;
  }
  .lead {
    font-size: var(--fs-metric-label);
    color: var(--dim);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    min-width: 48px;
    padding-bottom: var(--space-2);
  }
  .acts {
    display: flex;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .none,
  .note {
    margin: 0;
    font-size: var(--fs-desc);
    color: var(--dim);
  }
</style>
