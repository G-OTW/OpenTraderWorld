<script>
  // Top-bar theme control: cycles Light → Dark → System. Shows the icon of the
  // *active* theme; System is marked with a small "A" corner badge. Client-only
  // (localStorage), no backend — see $lib/theme/store.svelte.js.
  import Icon from '$lib/ui/Icon.svelte';
  import { theme } from '$lib/theme/store.svelte.js';
  import { t } from '$lib/i18n';

  const order = ['light', 'dark', 'system'];
  const label = $derived(
    theme.choice === 'system'
      ? $t('theme.system')
      : theme.choice === 'dark'
        ? $t('theme.dark')
        : $t('theme.light')
  );

  function cycle() {
    const i = order.indexOf(theme.choice);
    theme.set(order[(i + 1) % order.length]);
  }
</script>

<button class="theme-toggle" onclick={cycle} title={label} aria-label={label}>
  {#if theme.resolved === 'dark'}
    <Icon name="moon" size={16} />
  {:else}
    <Icon name="sun" size={16} />
  {/if}
  {#if theme.choice === 'system'}<span class="auto">A</span>{/if}
</button>

<style>
  .theme-toggle {
    position: relative;
    background: transparent;
    border: 1px solid transparent;
    color: var(--muted);
    width: 32px;
    height: 32px;
    border-radius: var(--radius);
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }
  .theme-toggle:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  /* Tiny badge marking "follow system". */
  .auto {
    position: absolute;
    right: 3px;
    bottom: 2px;
    font-size: 0.55rem;
    font-weight: var(--fw-semibold);
    line-height: 1;
    color: var(--accent);
    background: var(--surface);
    border-radius: 3px;
    padding: 0 1px;
  }
</style>
