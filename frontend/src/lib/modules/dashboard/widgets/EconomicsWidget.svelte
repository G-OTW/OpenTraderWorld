<script>
  // Economic-calendar widget — the same TradingView events embed as the economics module
  // page, sized by the widget cell instead of a fixed min-height. The injected iframe
  // scrolls internally, so even a compact cell stays usable.
  // Config: { importance } TradingView importanceFilter ('-1,0,1' | '0,1' | '1'),
  //         { countries } comma-separated country codes ('' = all).
  import { theme } from '$lib/theme/store.svelte.js';
  import { t } from '$lib/i18n';

  let { item, editing } = $props();

  let host = $state(null); // node the TradingView loader populates

  $effect(() => {
    const importanceFilter = item.config?.importance ?? '-1,0,1';
    const countryFilter = (item.config?.countries ?? '').trim();
    const colorTheme = theme.resolved;
    if (!host) return;
    host.innerHTML = '';
    const script = document.createElement('script');
    script.type = 'text/javascript';
    script.src = 'https://s3.tradingview.com/external-embedding/embed-widget-events.js';
    script.async = true;
    script.innerHTML = JSON.stringify({
      colorTheme,
      isTransparent: false,
      locale: 'en',
      countryFilter,
      importanceFilter,
      width: '100%',
      height: '100%'
    });
    host.appendChild(script);
    return () => {
      if (host) host.innerHTML = '';
    };
  });
</script>

{#if editing}
  <p class="hint">{$t('dashboard.widgets.economics.preview')}</p>
{:else}
  <div class="tv">
    <div class="tradingview-widget-container">
      <div class="tradingview-widget-container__widget" bind:this={host}></div>
    </div>
  </div>
{/if}

<style>
  .hint {
    color: var(--dim);
  }
  /* Bleed through the shell's body padding so the embed fills the whole cell. */
  .tv {
    height: calc(100% + 2 * var(--space-4));
    margin: calc(-1 * var(--space-4));
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .tradingview-widget-container {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .tradingview-widget-container__widget {
    flex: 1;
    min-height: 0;
  }
  :global(.tradingview-widget-container__widget iframe) {
    width: 100% !important;
    height: 100% !important;
  }
</style>
