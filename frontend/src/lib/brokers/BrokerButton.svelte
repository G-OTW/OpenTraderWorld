<script>
  // Shortcut to the account broker from inside a module: one button, its modal included.
  // `module` is the caller, so an account created from here is granted to it by default.
  import Icon from '$lib/ui/Icon.svelte';
  import BrokersModal from './BrokersModal.svelte';
  import { t } from '$lib/i18n';

  let { module = null, size = 'md', onchanged } = $props();
  let open = $state(false);
</script>

<button
  class="bbtn"
  class:sm={size === 'sm'}
  title={$t('brokers.manageTitle')}
  onclick={() => (open = true)}
>
  <Icon name="briefcase" size={size === 'sm' ? 12 : 14} /> {$t('brokers.manage')}
</button>

<BrokersModal bind:open {module} {onchanged} />

<style>
  .bbtn {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: transparent;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    /* Sits in rows of inputs and dropdowns, so it carries their height. */
    height: var(--control-h);
    padding: 0 var(--space-3);
    color: var(--text);
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    line-height: 1;
    cursor: pointer;
    transition:
      background-color 0.12s ease,
      border-color 0.12s ease;
  }
  .bbtn:hover {
    background: var(--surface-2);
  }
  .bbtn.sm {
    height: 24px;
    padding: 0 var(--space-3);
    border-radius: 0;
    font-size: var(--text-xs);
  }
</style>
