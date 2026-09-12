<script>
  // TradingView economic-calendar widget.
  //
  // TradingView embeds work by appending their loader <script> into a container; the
  // widget config is the script's text body. We can't render that script tag in Svelte
  // markup (it won't execute on hydration), so we build it imperatively on mount and
  // tear it down on destroy. Re-runs on theme change by rebuilding the widget.
  //
  // The "by TradingView" attribution link is required by their TOS — keep it.
  import { onMount } from 'svelte';

  let { colorTheme = 'dark' } = $props();

  let host; // the .tradingview-widget-container__widget node the loader populates

  function mountWidget() {
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
      countryFilter: '',
      importanceFilter: '-1,0,1',
      width: '100%',
      height: '100%'
    });
    host.appendChild(script);
  }

  onMount(() => {
    mountWidget();
    return () => {
      if (host) host.innerHTML = '';
    };
  });
</script>

<div class="tv">
  <div class="tradingview-widget-container">
    <div class="tradingview-widget-container__widget" bind:this={host}></div>
  </div>
</div>

<style>
  .tv {
    height: 100%;
    min-height: 0;
    display: flex;
    flex-direction: column;
    background: var(--surface);
    border: 0.5px solid var(--border);
    border-radius: 0;
    overflow: hidden;
  }
  .tradingview-widget-container {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  /* The loader injects an iframe in here; let it fill the space. */
  .tradingview-widget-container__widget {
    flex: 1;
    min-height: 480px;
  }
  :global(.tradingview-widget-container__widget iframe) {
    width: 100% !important;
    height: 100% !important;
  }
</style>
