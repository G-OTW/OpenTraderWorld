<script>
  // Shortcut to the data broker from inside a module: one button, its modal included.
  // Drop it in a page header or a picker; `module` is the caller, so a connector created
  // from here is granted to that module by default.
  import Icon from '$lib/ui/Icon.svelte';
  import ConnectorsModal from './ConnectorsModal.svelte';
  import { t } from '$lib/i18n';

  let { module = null, size = 'md', onchanged } = $props();
  let open = $state(false);
</script>

<button class="cbtn" class:sm={size === 'sm'} title={$t('connectors.manageTitle')} onclick={() => (open = true)}>
  <Icon name="plug" size={size === 'sm' ? 12 : 14} /> {$t('connectors.manage')}
</button>

<ConnectorsModal bind:open {module} {onchanged} />

<style>
  .cbtn {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: transparent;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    color: var(--text);
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    line-height: 1;
    cursor: pointer;
    transition: background-color 0.12s ease, border-color 0.12s ease;
  }
  .cbtn:hover {
    background: var(--surface-2);
  }
  .cbtn.sm {
    height: 24px;
    padding: 0 var(--space-3);
    border-radius: 0;
    font-size: var(--text-xs);
  }
</style>
